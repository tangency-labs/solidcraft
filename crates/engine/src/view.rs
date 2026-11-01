//! What the viewport shows, independent of the UI toolkit: body meshes, edges, sketches, the
//! grid and the origin. Used by the headless snapshot renderer; the GPU viewport draws the same
//! content.

use std::sync::Arc;

use solidcraft_geom::{Aabb3, Plane, Vec3};
use solidcraft_render::{AnalysisColors, Camera, Rgb, Scene, SceneAnalysis, SceneLine, SceneMesh};
use solidcraft_sketch::Sketch;

use crate::{Sel, Session};

/// Colours (shared with the GPU viewport).
pub mod colors {
    use solidcraft_render::Rgb;
    pub const BODY: Rgb = Rgb(176, 186, 198);
    pub const BODY_SELECTED: Rgb = Rgb(110, 170, 235);
    pub const EDGE: Rgb = Rgb(40, 44, 52);
    pub const SKETCH: Rgb = Rgb(30, 90, 200);
    pub const SKETCH_DONE: Rgb = Rgb(20, 20, 20);
    pub const SKETCH_CONSTRUCTION: Rgb = Rgb(230, 130, 40);
    pub const SKETCH_FIXED: Rgb = Rgb(20, 20, 20);
    /// Fixed (locked) geometry.
    pub const SKETCH_LOCKED: Rgb = Rgb(0, 140, 60);
    pub const SKETCH_CENTERLINE: Rgb = Rgb(200, 110, 60);
    /// Linked (projected) reference geometry.
    pub const SKETCH_PROJECTED: Rgb = Rgb(150, 60, 190);
    /// Linked geometry whose reference was lost.
    pub const SKETCH_LOST: Rgb = Rgb(200, 60, 90);
    pub const PROFILE: Rgb = Rgb(255, 196, 120);
    pub const GRID: Rgb = Rgb(196, 202, 212);
    pub const GRID_MAJOR: Rgb = Rgb(170, 178, 190);
    /// The origin axes: the light theme's X, Y and Z, the one set every axis in the UI is drawn
    /// with (#976).
    pub const AXIS_X: Rgb = Rgb(220, 60, 60);
    pub const AXIS_Y: Rgb = Rgb(60, 170, 60);
    pub const AXIS_Z: Rgb = Rgb(60, 100, 230);
    pub const BG_TOP: Rgb = Rgb(240, 243, 247);
    pub const BG_BOTTOM: Rgb = Rgb(203, 210, 221);
    /// What the section cap is tinted toward.
    pub const SECTION_CAP: Rgb = solidcraft_render::SECTION_CAP;
}

/// The colours a headless scene is drawn in. [`Palette::DEFAULT`] is [`colors`], the light
/// theme's look, which `solidcraft-cli snapshot`, MCP screenshots and thumbnails keep; the app
/// passes its theme's for `ui.render` and its CPU viewport, so they match the window. Only these
/// defaults change: a body's appearance and a face's own look are the design's, drawn as they are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// The background gradient, top and bottom.
    pub background: (Rgb, Rgb),
    pub grid: Rgb,
    pub grid_major: Rgb,
    /// Body edges, and the outline where the section cuts a body.
    pub edge: Rgb,
    /// A body without an appearance of its own.
    pub body: Rgb,
    pub body_selected: Rgb,
    /// Sketch curves: under-constrained, fully constrained (in the active sketch), in a finished
    /// sketch, fixed, projected, with a lost link, construction, centerline.
    pub sketch: Rgb,
    pub sketch_fixed: Rgb,
    pub sketch_done: Rgb,
    pub sketch_locked: Rgb,
    pub sketch_projected: Rgb,
    pub sketch_lost: Rgb,
    pub sketch_construction: Rgb,
    pub sketch_centerline: Rgb,
    /// The origin axes: X, Y, Z.
    pub axes: [Rgb; 3],
    /// What the section cap is tinted toward.
    pub cap: Rgb,
    /// The surface analysis colours.
    pub analysis: AnalysisColors,
}

impl Palette {
    /// [`colors`].
    pub const DEFAULT: Palette = Palette {
        background: (colors::BG_TOP, colors::BG_BOTTOM),
        grid: colors::GRID,
        grid_major: colors::GRID_MAJOR,
        edge: colors::EDGE,
        body: colors::BODY,
        body_selected: colors::BODY_SELECTED,
        sketch: colors::SKETCH,
        sketch_fixed: colors::SKETCH_FIXED,
        sketch_done: colors::SKETCH_DONE,
        sketch_locked: colors::SKETCH_LOCKED,
        sketch_projected: colors::SKETCH_PROJECTED,
        sketch_lost: colors::SKETCH_LOST,
        sketch_construction: colors::SKETCH_CONSTRUCTION,
        sketch_centerline: colors::SKETCH_CENTERLINE,
        axes: [colors::AXIS_X, colors::AXIS_Y, colors::AXIS_Z],
        cap: colors::SECTION_CAP,
        analysis: AnalysisColors::DEFAULT,
    };
}

impl Default for Palette {
    fn default() -> Self {
        Palette::DEFAULT
    }
}

/// Bounds of everything visible now, in world space: shown bodies where their occurrences,
/// joints and moves place them, and the sketches the view draws (not ones used by a feature,
/// hidden ones, or ones in hidden components). What Home, Fit and the orbit pivot frame.
pub fn bounds(s: &Session) -> Aabb3 {
    bounds_by(s, |body| body.mesh_bounds())
}

/// The Point Clouds folder's tree key (`Visibility::hidden_folders`).
pub const POINT_CLOUDS_FOLDER: &str = "c0/point_clouds";

/// The point clouds drawn now: shown, in a shown folder.
pub fn shown_point_clouds(s: &Session) -> impl Iterator<Item = &solidcraft_doc::point_cloud::PointCloud> {
    let folder = !s.folder_hidden(POINT_CLOUDS_FOLDER);
    s.doc.point_clouds.iter().filter(move |c| folder && c.visible)
}

/// [`bounds`] without meshing anything: a body not meshed yet counts with its outline
/// ([`solidcraft_kernel::Body::outline_bounds`]), which a face curving out past all of its edges
/// can exceed. Frames a design whose bodies are still being meshed.
pub fn unmeshed_bounds(s: &Session) -> Aabb3 {
    bounds_by(s, |body| body.cached_mesh().map(|m| m.bounds()).unwrap_or_else(|| body.outline_bounds()))
}

fn bounds_by(s: &Session, of: impl Fn(&solidcraft_doc::ModelBody) -> Aabb3) -> Aabb3 {
    let st = s.world_state();
    let mut b = Aabb3::EMPTY;
    for body in st.bodies.iter().filter(|x| s.body_visible(&x.name)) {
        b = b.union(&of(body));
    }
    for ss in st.sketches.iter().filter(|x| sketch_shown(s, x.feature)) {
        // All four corners: on a tilted plane two opposite ones don't span the rectangle.
        if let Some((lo, hi)) = ss.sketch.bounds() {
            for p in [lo, hi, solidcraft_geom::Vec2::new(lo.x, hi.y), solidcraft_geom::Vec2::new(hi.x, lo.y)] {
                b.add(ss.plane.to_world(p));
            }
        }
        for w in ss.sketch.wires.iter().filter(|w| !w.fit.is_empty() || !ss.sketch.view.hide_projected) {
            for p in &w.pts {
                b.add(*p);
            }
        }
    }
    for c in shown_point_clouds(s) {
        for p in c.world_corners().into_iter().flatten() {
            b.add(p);
        }
    }
    b
}

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

/// Sketch curves as world polylines with their colour.
pub fn sketch_lines(sk: &Sketch, plane: &Plane, active: bool, determined: &[bool]) -> Vec<(Vec<Vec3>, Rgb, bool)> {
    sketch_lines_with_sampler(sk, plane, active, determined, world_curve_parts)
}

/// A sketch curve as world polylines: the sampler of [`scene`] and [`sketch_lines`].
pub fn world_curve_parts(sk: &Sketch, i: usize, plane: &Plane) -> Vec<Vec<Vec3>> {
    sk.segs(i).into_iter().map(|seg| seg.polyline(0.02).iter().map(|p| plane.to_world(*p)).collect()).collect()
}

/// Sketch display paths with the same visibility and styling as [`sketch_lines`].
///
/// The sampler receives each visible 2D curve once and returns world-coordinate
/// parts. Parts remain separate, so a view-dependent sampler can leave clipping
/// gaps without drawing a line across them. Already placed 3D wires retain their
/// existing paths. Sampling changes only display geometry, never the sketch.
pub fn sketch_lines_with_sampler(
    sk: &Sketch,
    plane: &Plane,
    active: bool,
    determined: &[bool],
    sampler: impl FnMut(&Sketch, usize, &Plane) -> Vec<Vec<Vec3>>,
) -> Vec<(Vec<Vec3>, Rgb, bool)> {
    sketch_lines_in(sk, plane, active, determined, sampler, &Palette::DEFAULT)
}

/// [`sketch_lines_with_sampler`] in the colours of `p`.
fn sketch_lines_in(
    sk: &Sketch,
    plane: &Plane,
    active: bool,
    determined: &[bool],
    mut sampler: impl FnMut(&Sketch, usize, &Plane) -> Vec<Vec<Vec3>>,
    p: &Palette,
) -> Vec<(Vec<Vec3>, Rgb, bool)> {
    let mut out = Vec::new();
    for (i, c) in sk.curves.iter().enumerate() {
        if !sketch_curve_visible(sk, i) {
            continue;
        }
        let col = if c.link.is_some() && !sk.is_text_curve(i) {
            if sk.is_lost_curve(i) { p.sketch_lost } else { p.sketch_projected }
        } else if c.fixed {
            p.sketch_locked
        } else if c.centerline {
            p.sketch_centerline
        } else if c.construction {
            p.sketch_construction
        } else if !active {
            p.sketch_done
        } else if determined.get(i).copied().unwrap_or(false) {
            p.sketch_fixed
        } else {
            p.sketch
        };
        for part in sampler(sk, i, plane) {
            out.push((part, col, c.construction || c.centerline));
        }
    }
    // Virtual sharp corners (kept by fillets and chamfers): thin extension lines from the
    // curves to the corner, like construction geometry.
    for (_, ext) in sk.virtual_corners() {
        for (a, b) in ext {
            out.push((vec![plane.to_world(a), plane.to_world(b)], p.sketch_construction, true));
        }
    }
    // 3D curves are in world coordinates already.
    for w in sk.wires.iter().filter(|w| !w.fit.is_empty() || !sk.view.hide_projected) {
        // Curves drawn in a 3D sketch look like the sketch's own; included ones are references.
        let col = if w.fit.is_empty() {
            p.sketch_projected
        } else if active {
            p.sketch
        } else {
            p.sketch_done
        };
        out.push((w.pts.clone(), col, false));
    }
    out
}

/// Should sketch curve `i` be drawn and picked? The Sketch Palette's two geometry toggles each
/// hide one category, and a curve belongs to the category its colour already names in
/// [`sketch_lines_with_sampler`]: a linked curve is projected reference geometry (Projected
/// Geometries), anything else carrying the `construction` line type is construction geometry
/// (Construction Geometries). Text outlines are linked curves that the Text feature draws, so
/// they are never reference geometry and always show. Centerlines are their own line type and
/// no toggle hides them.
///
/// Drawing and picking share this one answer so they cannot drift: geometry that is not on
/// screen is never clickable, and geometry on screen always is.
pub fn sketch_curve_visible(sk: &Sketch, i: usize) -> bool {
    let Some(c) = sk.curves.get(i) else { return false };
    if c.link.is_some() && !sk.is_text_curve(i) {
        return !sk.view.hide_projected;
    }
    !(c.construction && sk.view.hide_construction)
}

/// Point roles shown and picked individually. Associated offsets keep their generated
/// fit nodes private until their association is broken, except an open offset's two ends,
/// which other curves snap and constrain to. Build once per sketch/frame.
pub struct SketchPointVisibility<'a> {
    hidden: std::collections::HashSet<&'a str>,
}

impl<'a> SketchPointVisibility<'a> {
    pub fn new(sk: &'a Sketch) -> Self {
        let outputs: std::collections::HashSet<&str> = sk.freeform_offsets.iter().map(|o| o.output.as_str()).collect();
        // A repeated link ID names its first link, as Sketch::link does.
        let mut seen = std::collections::HashSet::new();
        let text: std::collections::HashSet<&str> = sk
            .links
            .iter()
            .filter(|l| seen.insert(l.id.as_str()))
            .filter(|l| l.kind == solidcraft_sketch::LinkKind::Text)
            .map(|l| l.id.as_str())
            .collect();
        let mut hidden: std::collections::HashSet<&str> =
            sk.points.iter().filter(|p| p.link.as_deref().is_some_and(|l| text.contains(l))).map(|p| p.id.as_str()).collect();
        for curve in sk.curves.iter().filter(|c| outputs.contains(c.id.as_str())) {
            let ends = curve.kind.ends();
            let interior = curve.kind.point_ids().into_iter().filter(|i| ends.is_none_or(|(a, b)| *i != a && *i != b));
            hidden.extend(interior.filter_map(|i| sk.points.get(i).map(|p| p.id.as_str())));
        }
        Self { hidden }
    }

    pub fn takes(&self, point: &solidcraft_sketch::SPoint) -> bool {
        !self.hidden.contains(point.id.as_str())
    }
}

/// Should sketch point `i` be drawn or picked individually? Text outlines and derived
/// offset fit nodes (but an open offset's ends) are private. Batch callers use [`SketchPointVisibility`].
pub fn sketch_point_visible(sk: &Sketch, i: usize) -> bool {
    sk.points.get(i).is_some_and(|p| SketchPointVisibility::new(sk).takes(p))
}

/// Stable identification colour shared by every body of a component.
pub fn component_color(s: &Session, component: u64) -> Option<[u8; 3]> {
    s.colored_components?;
    Some(s.doc.component_identification_color(component))
}

/// The scene for a headless render, cut by the session's section.
pub fn scene(s: &Session, cam: &Camera) -> Scene {
    scene_with_section(s, cam, s.section)
}

/// The scene for a headless render, cut by `section` (origin, normal) instead of the session's:
/// the app passes the cut its viewport shows (an open Section Analysis dialog's preview, a
/// sketch's Slice), so model renders match the window.
pub fn scene_with_section(s: &Session, cam: &Camera, section: Option<(Vec3, Vec3)>) -> Scene {
    scene_with_guides(s, cam, section, Guides::default())
}

/// The plane of the sketch that is open, if any: it always shows a grid, whatever the Grid switch
/// says (outside a sketch the grid is the user's choice).
pub fn open_sketch_plane(s: &Session) -> Option<Plane> {
    let id = s.active_sketch?;
    s.world_state().sketch(id).map(|ss| ss.plane)
}

/// The grid on a sketch plane: its centre (the view target's foot on the plane, snapped to the
/// major step in the plane's own axes) and the plane's x and y directions.
pub fn sketch_grid_frame(plane: &Plane, target: Vec3, major: f64) -> (Vec3, Vec3, Vec3) {
    let snap = |d: Vec3| d * (((target - plane.origin).dot(d) / major).round() * major);
    (plane.origin + snap(plane.x) + snap(plane.y), plane.x, plane.y)
}

/// What a headless render draws besides the model: the ground grid and the origin's axes.
/// Both are on by default; the grid lies on Z 0, so it cuts through a model that straddles it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guides {
    /// The ground grid.
    pub grid: bool,
    /// The origin's axes. An axis hidden with `browser.visibility` (`X`, `Y`, `Z`, or the whole
    /// `origin` folder) stays hidden whatever this says, as in the viewport.
    pub axes: bool,
}

impl Default for Guides {
    fn default() -> Self {
        Guides { grid: true, axes: true }
    }
}

/// [`scene_with_section`] with the grid and the axes shown or left out.
pub fn scene_with_guides(s: &Session, cam: &Camera, section: Option<(Vec3, Vec3)>, guides: Guides) -> Scene {
    scene_with_palette_and_guides(s, cam, section, world_curve_parts, &Palette::DEFAULT, guides)
}

/// A headless scene using supplied display paths for its visible sketch curves.
/// Callers can use the same pixel sampler as a GPU viewport without changing the
/// body, grid, appearance, section or sketch styling policies of [`scene`].
pub fn scene_with_sketch_sampler(s: &Session, cam: &Camera, sampler: impl FnMut(&Sketch, usize, &Plane) -> Vec<Vec<Vec3>>) -> Scene {
    scene_with_section_and_sketch_sampler(s, cam, s.section, sampler)
}

/// A headless scene with an explicit section and supplied sketch display paths.
pub fn scene_with_section_and_sketch_sampler(
    s: &Session,
    cam: &Camera,
    section: Option<(Vec3, Vec3)>,
    sampler: impl FnMut(&Sketch, usize, &Plane) -> Vec<Vec<Vec3>>,
) -> Scene {
    scene_with_palette(s, cam, section, sampler, &Palette::DEFAULT)
}

/// [`scene_with_section_and_sketch_sampler`] in the colours of `p` (the app's theme).
pub fn scene_with_palette(
    s: &Session,
    cam: &Camera,
    section: Option<(Vec3, Vec3)>,
    sampler: impl FnMut(&Sketch, usize, &Plane) -> Vec<Vec<Vec3>>,
    p: &Palette,
) -> Scene {
    build_scene(s, cam, section, Guides::default(), sampler, p, false)
}

/// [`scene_with_palette`] with the ground grid and the origin's axes shown or left out.
pub fn scene_with_palette_and_guides(
    s: &Session,
    cam: &Camera,
    section: Option<(Vec3, Vec3)>,
    sampler: impl FnMut(&Sketch, usize, &Plane) -> Vec<Vec<Vec3>>,
    p: &Palette,
    guides: Guides,
) -> Scene {
    build_scene(s, cam, section, guides, sampler, p, false)
}

/// A picture of the view with cosmetic thread markings explicitly enabled or disabled.
/// The ordinary scene helpers leave these optional markings off.
pub fn scene_with_thread_markings(
    s: &Session,
    cam: &Camera,
    section: Option<(Vec3, Vec3)>,
    sampler: impl FnMut(&Sketch, usize, &Plane) -> Vec<Vec<Vec3>>,
    p: &Palette,
    guides: Guides,
    cosmetic_threads: bool,
) -> Scene {
    build_scene(s, cam, section, guides, sampler, p, cosmetic_threads)
}

fn build_scene(
    s: &Session,
    cam: &Camera,
    section: Option<(Vec3, Vec3)>,
    guides: Guides,
    mut sampler: impl FnMut(&Sketch, usize, &Plane) -> Vec<Vec<Vec3>>,
    p: &Palette,
    cosmetic_threads: bool,
) -> Scene {
    let st = s.world_state();
    let section = section.filter(|(o, n)| o.is_finite() && n.is_finite() && n.len() > 1e-9);
    let mut sc = Scene {
        background: Some(p.background),
        clip: section.map(|(o, n)| (n, n.dot(o))),
        cap: Some(p.cap),
        plain_cap: !s.section_hatch,
        analysis: s.analysis.map(|a| SceneAnalysis { mode: a.mode(), colors: p.analysis }),
        ..Default::default()
    };
    let b = bounds(s);
    sc.radius = (b.diagonal() * 0.5).max(cam.half_height()).max(10.0) + b.center().dist(cam.target);
    // Grid on the ground plane (XY with Z up, XZ with Y up), drawn under the model as in the viewport.
    if guides.grid {
        let major = grid_step(cam.half_height()).1;
        // Headless pictures keep the grid on the ground, the viewport's default; an open sketch
        // has its own plane's grid (world coordinates, no camera frame).
        let (c, u, v, slant, w): (Vec3, Vec3, Vec3, f64, Box<dyn Fn(Vec3) -> Vec3>) = if let Some(pl) = open_sketch_plane(s) {
            let (c, u, v) = sketch_grid_frame(&pl, cam.target, major);
            let n = u.cross(v);
            (c, u, v, cam.back().dot(n), Box::new(|p| p))
        } else {
            let tf = cam.to_frame(cam.target);
            let (u, v) = (Vec3::X, Vec3::Y);
            let back = cam.to_frame(cam.back());
            let n = u.cross(v);
            let snap = |a: Vec3| a * ((tf.dot(a) / major).round() * major);
            (snap(u) + snap(v), u, v, back.dot(n), Box::new(|p| cam.from_frame(p)))
        };
        // The grid reaches as far on the ground as the view is wide (the ground stretches as it
        // is seen at a slant) and fades out with the distance, so that it dissolves into the
        // distance instead of ending in a cut (#4214).
        let ext = grid_ext(cam.half_height(), major, slant);
        let far_reach = grid_far_reach(cam, cam.clip(sc.radius).1, w(c), w(u.cross(v)));
        let fade = GridFade::new(cam.half_height(), slant, ext, far_reach);
        // The grid's centre on each axis, from the plane's origin (the world's, or the sketch
        // plane's): a level fading in near a switch of the spacing lies on its own places (#9570).
        let origin = open_sketch_plane(s).map_or(Vec3::ZERO, |pl| pl.origin);
        let (at_u, at_v) = ((c - origin).dot(u), (c - origin).dot(v));
        // Two lines through the point `c + u * t`: one along v, one along u.
        for (along, across, at) in [(v, u, at_u), (u, v, at_v)] {
            for line in grid_lines(cam.half_height(), at, fade.end, 400) {
                let t = line.offset;
                // The viewport's widths (pixels). The headless picture keeps the palette's two
                // grid colours: a line two levels share with different kinds is drawn as both,
                // each as strong as its share.
                let width = 0.7 + 0.3 * f64::from(line.major);
                for (col, share) in [(p.grid_major, line.major), (p.grid, 1.0 - line.major)] {
                    if share <= 0.0 {
                        continue;
                    }
                    for (s0, s1, alpha) in fade.pieces(t) {
                        sc.grid.push(SceneLine {
                            points: vec![w(c + across * t + along * s0), w(c + across * t + along * s1)],
                            color: col,
                            alpha: alpha * line.weight * share,
                            width,
                            on_top: false,
                        });
                    }
                }
            }
        }
    }
    // The origin's axes, each unless hidden one by one (`browser.visibility`), as in the viewport.
    let axis = cam.half_height() * 0.25;
    for ((name, d), col) in [("X", Vec3::X), ("Y", Vec3::Y), ("Z", Vec3::Z)].into_iter().zip(p.axes) {
        if guides.axes && !s.visibility.hidden_origin.iter().any(|h| h == name) {
            sc.lines.push(SceneLine { points: vec![Vec3::ZERO, d * axis], color: col, alpha: 1.0, width: 1.6, on_top: false });
        }
    }
    let selected_occurrences: Vec<String> = s
        .selection
        .iter()
        .filter_map(|x| match x {
            Sel::Occurrence { path } => s.occurrence_bodies(path, true).ok(),
            _ => None,
        })
        .flatten()
        .collect();
    let mut thread_budget = crate::cosmetic_threads::Budget::default();
    for body in st.bodies.iter().filter(|b| s.body_visible(&b.name)) {
        let selected = selected_occurrences.contains(&body.name) || s.selection.iter().any(|x| matches!(x, Sel::Body { name } if *name == body.name));
        let mesh = body.mesh();
        // The body's appearance (or imported colour) and faces with looks of their own, as in
        // the viewport.
        let to8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        let color = if selected {
            p.body_selected
        } else if let Some([r, g, b]) = component_color(s, body.component) {
            Rgb(r, g, b)
        } else {
            s.doc.body_color(body).map_or(p.body, |[r, g, b]| Rgb(to8(r), to8(g), to8(b)))
        };
        let face_colors = if selected || s.colored_components.is_some() {
            Vec::new()
        } else {
            s.doc.face_colors(body).into_iter().map(|(f, l)| (f as u32, Rgb(l.color[0], l.color[1], l.color[2]))).collect()
        };
        // A see-through appearance is drawn see-through, as the viewport draws it; a selected
        // body is drawn solid, so the selection colour reads.
        let opacity = if selected { 1.0 } else { s.doc.model_body_opacity(body) as f32 };
        sc.meshes.push(SceneMesh { mesh: Arc::clone(&mesh), color, face_colors, opacity });
        if cosmetic_threads
            && thread_budget.available()
            && s.doc.model_body_opacity(body) >= 0.999
            && s.doc.face_colors(body).iter().all(|(_, l)| l.opacity >= 0.999)
        {
            for (a, b) in crate::cosmetic_threads::segments_with_budget(body, &mesh, &st.threads, &mut thread_budget) {
                sc.lines.push(SceneLine { points: vec![a, b], color: p.edge, alpha: 1.0, width: 0.85, on_top: false });
            }
        }
        for (i, e) in mesh.edges.iter().enumerate() {
            if !mesh.shaded_edge_visible(i) {
                continue;
            }
            sc.lines.push(SceneLine { points: e.clone(), color: p.edge, alpha: 1.0, width: 1.2, on_top: false });
        }
        // The outline of the body where the section cuts it.
        if let Some((o, n)) = section {
            for (a, b) in section_segments(&mesh, o, n) {
                sc.lines.push(SceneLine { points: vec![a, b], color: p.edge, alpha: 1.0, width: 1.6, on_top: false });
            }
        }
    }
    for ss in &st.sketches {
        let active = s.active_sketch == Some(ss.feature);
        if !sketch_shown(s, ss.feature) {
            continue;
        }
        for (pts, col, cons) in sketch_lines_in(&ss.sketch, &ss.plane, active, &ss.report.curve_determined, &mut sampler, p) {
            sc.lines.push(SceneLine { points: pts, color: col, alpha: 1.0, width: if cons { 1.0 } else { 1.6 }, on_top: active });
        }
    }
    sc
}

/// Where the plane through `o` with normal `n` cuts a mesh: one segment per crossed triangle,
/// nudged a hair to the kept side (so the clip leaves them drawn).
pub fn section_segments(m: &solidcraft_geom::Mesh, o: Vec3, n: Vec3) -> Vec<(Vec3, Vec3)> {
    let eps = section_nudge(m);
    let mut out = Vec::new();
    for t in &m.triangles {
        cut_triangle(m, t, o, n, eps, &mut out);
    }
    out
}

/// How far [`section_segments`] nudges the cut to the kept side.
fn section_nudge(m: &solidcraft_geom::Mesh) -> f64 {
    1e-4 * (1.0 + m.bounds().diagonal())
}

/// The segment where the plane through `o` with normal `n` crosses triangle `t`, if it does.
fn cut_triangle(m: &solidcraft_geom::Mesh, t: &[u32; 3], o: Vec3, n: Vec3, eps: f64, out: &mut Vec<(Vec3, Vec3)>) {
    let Some(p) = m.tri(t) else { return };
    let d = p.map(|q| n.dot(q - o));
    let mut pts = Vec::with_capacity(2);
    for (i, j) in [(0, 1), (1, 2), (2, 0)] {
        if (d[i] < 0.0) != (d[j] < 0.0) {
            let s = d[i] / (d[i] - d[j]);
            pts.push(p[i] + (p[j] - p[i]) * s - n * eps);
        }
    }
    if let [a, b] = pts[..] {
        out.push((a, b));
    }
}

/// A mesh's triangles sorted into the cells of a grid by where their boxes sit, each cell
/// keeping the box around its triangles, so a cut with any plane looks only at the triangles of
/// the cells the plane crosses: an imported part can have millions of triangles, and dragging a
/// Section Analysis moves the cut and, when tilted, turns its normal every frame (#10373). The
/// same segments, in the same order, as [`section_segments`].
pub struct SectionIndex {
    /// Per cell holding triangles, the centre and half size of the box around them, and where
    /// they are in `tris` (in mesh order).
    cells: Vec<(Vec3, Vec3, u32, u32)>,
    tris: Vec<u32>,
    /// Triangles with a corner that is not finite: cut on every plane, as `section_segments`
    /// does.
    always: Vec<u32>,
    /// The largest coordinate of the mesh, for the cell test's rounding margin.
    reach: f64,
    eps: f64,
}

/// How many mean triangle widths a [`SectionIndex`] cell is wide, and most cells of its grid.
const SECTION_CELL_WIDTHS: f64 = 4.0;
const SECTION_GRID: usize = 1 << 22;

impl SectionIndex {
    pub fn new(m: &solidcraft_geom::Mesh) -> SectionIndex {
        let finite_tri = |t: &[u32; 3]| m.tri(t).filter(|p| p.iter().all(|q| q.is_finite()));
        let mut always = Vec::new();
        let mut centres = Aabb3::EMPTY;
        let (mut widths, mut count) = (0.0, 0usize);
        for (i, t) in m.triangles.iter().enumerate() {
            match finite_tri(t) {
                Some([a, b, c]) => {
                    let (lo, hi) = (a.min(b).min(c), a.max(b).max(c));
                    centres.add((lo + hi) * 0.5);
                    widths += (hi - lo).len();
                    count += 1;
                }
                None if m.tri(t).is_some() => always.extend(u32::try_from(i)),
                None => {}
            }
        }
        // Cells as wide as a few triangles, on the axes the part spans: a part's triangles lie
        // on its surface, so cells sized from the part's volume would hold far more.
        let size = if centres.is_empty() { Vec3::ZERO } else { centres.size() };
        let ext = [size.x, size.y, size.z];
        let dims_for = |side: f64| ext.map(|e| if e > 0.0 && side > 0.0 { (e / side).ceil().clamp(1.0, SECTION_GRID as f64) as usize } else { 1 });
        let cells_of = |d: [usize; 3]| d.iter().fold(1usize, |p, k| p.saturating_mul(*k));
        let mut side = widths / count.max(1) as f64 * SECTION_CELL_WIDTHS;
        while cells_of(dims_for(side)) > SECTION_GRID && side.is_finite() {
            side *= 1.25;
        }
        let dims = dims_for(side);
        let cells = cells_of(dims).min(SECTION_GRID);
        let at = |v: f64, lo: f64, e: f64, d: usize| {
            let k = if e > 0.0 { ((v - lo) / e * d as f64).floor() } else { 0.0 };
            if k.is_finite() { (k.max(0.0) as usize).min(d.saturating_sub(1)) } else { 0 }
        };
        let of: Vec<Option<usize>> = m
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = finite_tri(t)?;
                let p = (a.min(b).min(c) + a.max(b).max(c)) * 0.5;
                let [x, y, z] =
                    [at(p.x, centres.min.x, ext[0], dims[0]), at(p.y, centres.min.y, ext[1], dims[1]), at(p.z, centres.min.z, ext[2], dims[2])];
                Some(((z * dims[1] + y) * dims[0] + x).min(cells.saturating_sub(1)))
            })
            .collect();
        // Counted per cell, then each cell's first slot (the last entry: the total).
        let mut starts = vec![0u32; cells + 1];
        for c in of.iter().flatten() {
            if let Some(s) = starts.get_mut(*c) {
                *s += 1;
            }
        }
        let mut total = 0u32;
        for s in &mut starts {
            (*s, total) = (total, total.saturating_add(*s));
        }
        let mut fill = starts.clone();
        let mut tris = vec![0u32; total as usize];
        for (i, c) in of.iter().enumerate() {
            let (Some(c), Ok(i)) = (c, u32::try_from(i)) else { continue };
            if let Some(at) = fill.get_mut(*c) {
                if let Some(slot) = tris.get_mut(*at as usize) {
                    *slot = i;
                }
                *at += 1;
            }
        }
        let mut reach: f64 = 0.0;
        let cells = starts
            .windows(2)
            .filter_map(|w| match *w {
                [a, e] if e > a => {
                    let mut b = Aabb3::EMPTY;
                    for i in tris.get(a as usize..e as usize).unwrap_or(&[]) {
                        for p in m.triangles.get(*i as usize).and_then(|t| m.tri(t)).into_iter().flatten() {
                            b.add(p);
                        }
                    }
                    reach = reach.max(b.min.x.abs().max(b.min.y.abs()).max(b.min.z.abs()).max(b.max.x.abs()).max(b.max.y.abs()).max(b.max.z.abs()));
                    Some((b.center(), b.size() * 0.5, a, e))
                }
                _ => None,
            })
            .collect();
        SectionIndex { cells, tris, always, reach, eps: section_nudge(m) }
    }

    /// [`section_segments`] of `m` (the mesh this index was made from) at the plane through
    /// `o` with normal `n`; nothing for a plane that is not finite.
    pub fn segments(&self, m: &solidcraft_geom::Mesh, o: Vec3, n: Vec3) -> Vec<(Vec3, Vec3)> {
        if !(o.is_finite() && n.is_finite()) {
            return Vec::new();
        }
        let an = Vec3::new(n.x.abs(), n.y.abs(), n.z.abs());
        // Rounding in `cut_triangle`'s heights is far below this margin, so a cell left out
        // holds no triangle it would cut.
        let margin = 1e-9 * (an.x + an.y + an.z) * (self.reach + o.x.abs().max(o.y.abs()).max(o.z.abs()) + 1.0);
        let at = n.dot(o);
        // The cut triangles' segments, by triangle, then in mesh order.
        let mut found: Vec<(u32, (Vec3, Vec3))> = Vec::new();
        let mut out = Vec::with_capacity(1);
        let mut cut = |i: u32| {
            if let Some(t) = m.triangles.get(i as usize) {
                cut_triangle(m, t, o, n, self.eps, &mut out);
                found.extend(out.pop().map(|seg| (i, seg)));
            }
        };
        self.always.iter().for_each(|i| cut(*i));
        for (centre, half, first, end) in &self.cells {
            if (n.dot(*centre) - at).abs() <= an.dot(*half) + margin {
                self.tris.get(*first as usize..*end as usize).unwrap_or(&[]).iter().for_each(|i| cut(*i));
            }
        }
        found.sort_unstable_by_key(|(i, _)| *i);
        found.into_iter().map(|(_, seg)| seg).collect()
    }
}

/// Is a sketch drawn? The sketch being edited always is; otherwise not in a hidden component,
/// Sketches folder or group (even one shown on its own). The sketch immediately before a
/// rollback marker is shown regardless of its individual visibility; other hidden sketches, and
/// ones used by a feature unless shown on their own, retain their stored visibility.
pub fn sketch_shown(s: &Session, id: u64) -> bool {
    let v = &s.visibility;
    if s.active_sketch == Some(id) || s.feature_edit_sketch == Some(id) {
        return true;
    }
    if !s.feature_placement_visible(id) || s.feature_folder_hidden(id, "sketches") {
        return false;
    }
    if s.doc
        .marker
        .and_then(|position| position.checked_sub(1))
        .and_then(|index| s.doc.features.get(index))
        .is_some_and(|f| f.id == id && matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. }))
    {
        return true;
    }
    if v.hidden_sketches.contains(&id) {
        return false;
    }
    v.shown_sketches.contains(&id) || !sketch_consumed(s, id)
}

/// Is a sketch used by a later feature? A sheet metal base or contour flange uses its sketch as
/// an Extrude does: shown, its lines lie on the sheet's edges and take the clicks meant for
/// them (#6804). So does a Hole at sketch points: shown, its points are drawn in the holes
/// (#9082). A Single hole snapped to one point (#2110) does not: it was placed on a face, not
/// built from the sketch, and hiding the sketch would take its other points from the next
/// Single hole's snap.
pub fn sketch_consumed(s: &Session, id: u64) -> bool {
    use solidcraft_doc::{FeatureKind, SketchPoints};
    s.doc.features.iter().any(|f| {
        matches!(f.kind, FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. }
            | FeatureKind::SheetBase { sketch, .. } | FeatureKind::SheetContour { sketch, .. }
            | FeatureKind::Hole { points: Some(SketchPoints { sketch, single: false, .. }), .. } if sketch == id)
    })
}

/// Camera framing the model (iso view).
pub fn home_camera(s: &Session) -> Camera {
    let mut c = Camera::default();
    let b = bounds(s);
    if !b.is_empty() {
        c.fit(&b);
    } else {
        c.distance = 150.0;
    }
    c
}

/// Construction planes of the timeline, as evaluated: (feature id, name, plane).
pub fn construction_planes(s: &Session) -> Vec<(u64, String, Plane)> {
    constructs(s)
        .into_iter()
        .filter_map(|(id, name, g)| match g {
            solidcraft_doc::construct::ConstructGeom::Plane(p) => Some((id, name, p)),
            _ => None,
        })
        .collect()
}

/// Construction axes, as evaluated: (feature id, name, origin, unit direction).
pub fn construction_axes(s: &Session) -> Vec<(u64, String, Vec3, Vec3)> {
    constructs(s)
        .into_iter()
        .filter_map(|(id, name, g)| match g {
            solidcraft_doc::construct::ConstructGeom::Axis { origin, dir } => Some((id, name, origin, dir)),
            _ => None,
        })
        .collect()
}

/// Construction points, as evaluated: (feature id, name, position).
pub fn construction_points(s: &Session) -> Vec<(u64, String, Vec3)> {
    constructs(s)
        .into_iter()
        .filter_map(|(id, name, g)| match g {
            solidcraft_doc::construct::ConstructGeom::Point(p) => Some((id, name, p)),
            _ => None,
        })
        .collect()
}

/// Construction geometry of unsuppressed features, shown where its component is placed.
fn constructs(s: &Session) -> Vec<(u64, String, solidcraft_doc::construct::ConstructGeom)> {
    use solidcraft_doc::construct::ConstructGeom;
    let st = s.model.state();
    st.construct
        .iter()
        .filter_map(|c| {
            let f = s.doc.feature(c.feature).filter(|f| !f.suppressed)?;
            let m = s.doc.component_transform(f.component);
            if solidcraft_doc::is_identity(&m) {
                return Some((c.feature, c.name.clone(), c.geom.clone()));
            }
            let (pt, v) = (|p: Vec3| solidcraft_doc::apply_point(&m, p), |d: Vec3| solidcraft_doc::apply_vector(&m, d));
            let g = match &c.geom {
                ConstructGeom::Plane(p) => ConstructGeom::Plane(Plane::new(pt(p.origin), v(p.x), v(p.y)).unwrap_or(*p)),
                ConstructGeom::Axis { origin, dir } => ConstructGeom::Axis { origin: pt(*origin), dir: v(*dir) },
                ConstructGeom::Point(p) => ConstructGeom::Point(pt(*p)),
            };
            Some((c.feature, c.name.clone(), g))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Headless pictures show appearances as the viewport does: a body's colour and faces with
    /// looks of their own (the picture itself: `solidcraft-gpu`'s capture tests).
    #[test]
    fn snapshot_scene_carries_appearances() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        s.execute("solid.cylinder", &json!({"diameter": 14, "height": 30, "base": [50, 15, 0], "operation": "new"})).unwrap();
        s.execute("appearance.assign", &json!({"bodies": ["Body2"], "color": [220, 60, 40]})).unwrap();
        s.execute("appearance.assign", &json!({"faces": [[20, 15, 20]], "body": "Body1", "color": [40, 170, 80]})).unwrap();
        let cam = home_camera(&s);
        let sc = scene(&s, &cam);
        assert!(sc.meshes.iter().any(|m| m.color == Rgb(220, 60, 40)));
        assert!(sc.meshes.iter().any(|m| m.face_colors.iter().any(|(_, c)| *c == Rgb(40, 170, 80))));
    }

    /// A render can leave out the ground grid and the origin's axes (a box straddling Z 0 is cut
    /// through by the grid), each on its own; the body's own lines stay.
    #[test]
    fn guides_leave_the_grid_and_the_axes_out() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20, "corner": [10, -15, -10]})).unwrap();
        let cam = home_camera(&s);
        let grid = [colors::GRID, colors::GRID_MAJOR];
        let axes = [colors::AXIS_X, colors::AXIS_Y, colors::AXIS_Z];
        let count = |sc: &Scene, colours: &[Rgb]| sc.grid.iter().chain(&sc.lines).filter(|l| colours.contains(&l.color)).count();
        let all = scene(&s, &cam);
        let (grid_lines, edges) = (count(&all, &grid), count(&all, &[colors::EDGE]));
        assert!(grid_lines > 20 && edges >= 12, "{grid_lines} grid lines, {edges} edges");
        assert_eq!(count(&all, &axes), 3);
        assert_eq!(all.lines.len(), scene_with_guides(&s, &cam, s.section, Guides::default()).lines.len());
        let render = |g: Guides| scene_with_guides(&s, &cam, s.section, g);
        let no_grid = render(Guides { grid: false, axes: true });
        assert_eq!((count(&no_grid, &grid), count(&no_grid, &axes), count(&no_grid, &[colors::EDGE])), (0, 3, edges));
        let no_axes = render(Guides { grid: true, axes: false });
        assert_eq!((count(&no_axes, &grid), count(&no_axes, &axes), count(&no_axes, &[colors::EDGE])), (grid_lines, 0, edges));
        let bare = render(Guides { grid: false, axes: false });
        assert_eq!((bare.lines.len(), bare.grid.len()), (edges, 0));
    }

    /// An origin axis hidden with `browser.visibility` stays out of headless renders, as it does
    /// out of the viewport, whatever the render's own switch says; the grid is not the origin.
    #[test]
    fn hidden_origin_axes_stay_out_of_renders() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let cam = home_camera(&s);
        let axes = [colors::AXIS_X, colors::AXIS_Y, colors::AXIS_Z];
        let shown = |s: &Session| -> Vec<Rgb> {
            let sc = scene(s, &cam);
            axes.into_iter().filter(|c| sc.lines.iter().any(|l| l.color == *c)).collect()
        };
        assert_eq!(shown(&s), axes);
        s.execute("browser.visibility", &json!({"items": ["Y"], "visible": false})).unwrap();
        assert_eq!(shown(&s), [colors::AXIS_X, colors::AXIS_Z]);
        s.execute("browser.visibility", &json!({"folder": "origin", "visible": false})).unwrap();
        assert!(shown(&s).is_empty());
        let sc = scene_with_guides(&s, &cam, None, Guides { grid: true, axes: true });
        assert!(sc.grid.iter().any(|l| l.color == colors::GRID), "the grid is not part of the origin");
        s.execute("browser.visibility", &json!({"folder": "origin", "visible": true})).unwrap();
        assert_eq!(shown(&s), axes);
    }

    /// A section cuts the headless scene: the clip plane and the cap colour, and the outline
    /// where the plane cuts each body (the picture: `solidcraft-gpu`'s capture tests).
    #[test]
    fn the_section_cuts_the_scene() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let cam = home_camera(&s);
        assert!(scene(&s, &cam).clip.is_none());
        s.execute("inspect.section", &json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
        let sc = scene(&s, &cam);
        let (n, d) = sc.clip.unwrap();
        assert!((n - Vec3::new(0.0, -1.0, 0.0)).len() < 1e-12 && (d + 15.0).abs() < 1e-12, "{n:?} {d}");
        assert_eq!(sc.cap, Some(colors::SECTION_CAP));
        assert!(!section_segments(&s.world_state().bodies[0].mesh(), Vec3::new(0.0, 15.0, 0.0), -Vec3::Y).is_empty());
    }

    /// The surface analysis shades the headless scene too, in the palette's colours.
    #[test]
    fn the_surface_analysis_shades_the_scene() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let cam = home_camera(&s);
        assert!(scene(&s, &cam).analysis.is_none());
        s.analysis = Some(crate::SurfaceAnalysis::Curvature { radius: 5.0 });
        let a = scene(&s, &cam).analysis.unwrap();
        assert_eq!((a.mode, a.colors), (solidcraft_render::AnalysisMode::Curvature { radius: 5.0 }, Palette::DEFAULT.analysis));
    }

    /// The CPU rasterizer (the last tier of headless pictures, `SOLIDCRAFT_GPU=cpu` or no adapter
    /// under `auto`): curved surfaces get a dark silhouette line where they turn away.
    #[test]
    fn cpu_pictures_draw_silhouettes_on_curved_surfaces() {
        let mut s = Session::default();
        s.execute("solid.sphere", &json!({"diameter": 20})).unwrap();
        let cam = home_camera(&s);
        let img = solidcraft_render::render(&scene(&s, &cam), &cam, 400, 400);
        let dark = img.rgba.chunks(4).filter(|p| p[0] < 70 && p[1] < 70 && p[2] < 75).count();
        assert!(dark > 40, "silhouette pixels: {dark}");
    }

    /// The CPU rasterizer: a section shows; the cut part is gone and the cut face is drawn.
    #[test]
    fn cpu_pictures_show_the_section() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let cam = home_camera(&s);
        let whole = solidcraft_render::render(&scene(&s, &cam), &cam, 200, 140).rgba;
        s.execute("inspect.section", &json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
        let sc = scene(&s, &cam);
        assert!(sc.clip.is_some());
        let cut = solidcraft_render::render(&sc, &cam, 200, 140).rgba;
        let changed = whole.chunks(4).zip(cut.chunks(4)).filter(|(a, b)| a != b).count();
        assert!(changed > 500, "{changed} pixels changed");
    }

    /// The section's cut face hides what lies inside the body behind it (it is drawn at the
    /// plane's depth, not on the far wall like a pane of glass), shows in the cap colour, and
    /// the cut's outline is drawn.
    #[test]
    fn cpu_picture_section_caps_hide_the_inside() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        // Cut away the front half (y < 15), so the iso camera looks into the cut.
        s.execute("inspect.section", &json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
        for fov in [0.0, 0.6] {
            let mut cam = home_camera(&s);
            cam.fov = fov;
            let mut sc = scene(&s, &cam);
            assert!(!section_segments(&s.world_state().bodies[0].mesh(), Vec3::new(0.0, 15.0, 0.0), -Vec3::Y).is_empty());
            // A marker inside the body, behind the cut face.
            let red = Rgb(255, 0, 0);
            sc.lines.push(SceneLine {
                points: vec![Vec3::new(5.0, 25.0, 10.0), Vec3::new(35.0, 25.0, 10.0)],
                color: red,
                alpha: 1.0,
                width: 3.0,
                on_top: false,
            });
            let img = solidcraft_render::render(&sc, &cam, 300, 220);
            let px = || img.rgba.chunks(4);
            let marker = px().filter(|p| p[0] > 200 && p[1] < 60 && p[2] < 60).count();
            assert_eq!(marker, 0, "fov {fov}: the inside shows through the cut face");
            // The cap: warm (red over blue), unlike the grey-blue body and background.
            let cap = px().filter(|p| p[0] as i32 - p[2] as i32 > 20).count();
            assert!(cap > 500, "fov {fov}: cap pixels {cap}");
        }
    }

    /// The cut faces are hatched (stripes over the body's colour tinted toward the cap colour)
    /// unless the section's `hatch` is off; then the cut is the body's own solid colour.
    #[test]
    fn cpu_picture_section_hatch_is_optional() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        s.execute("appearance.assign", &json!({"bodies": ["Body1"], "color": [40, 160, 90]})).unwrap();
        let cam = home_camera(&s);
        let mut picture = |hatch: bool| {
            let plane = json!({"origin": [0, 15, 0], "normal": [0, -1, 0]});
            s.execute("inspect.section", &json!({"plane": plane, "hatch": hatch})).unwrap();
            solidcraft_render::render(&scene(&s, &cam), &cam, 300, 220).rgba
        };
        // Pixels in the body's exact colour: only a plain cut has them (lit faces are shaded).
        let solid = |img: &[u8]| img.chunks(4).filter(|p| p[..3] == [40, 160, 90]).count();
        let (hatched, plain) = (picture(true), picture(false));
        assert!(solid(&plain) > 500, "plain cut: {} pixels in the body's colour", solid(&plain));
        assert_eq!(solid(&hatched), 0, "the hatched cut is tinted");
        let shades = |img: &[u8]| img.chunks(4).map(|p| [p[0], p[1], p[2]]).collect::<std::collections::BTreeSet<_>>().len();
        assert!(shades(&hatched) > shades(&plain), "the hatch adds its stripes' shades");
    }
}

#[cfg(test)]
#[path = "view_display_tests.rs"]
mod display_tests;

#[cfg(test)]
#[path = "view_palette_tests.rs"]
mod palette_tests;
