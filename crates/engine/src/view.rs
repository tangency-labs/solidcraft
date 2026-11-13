//! What the viewport shows, independent of the UI toolkit: body meshes, edges, sketches, the
//! grid and the origin. Used by the headless snapshot renderer; the GPU viewport draws the same
//! content.

use solidcraft_geom::prelude::*;
use std::sync::Arc;

use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_color::AnalysisColors;
use solidcraft_geom::Aabb3;
use solidcraft_sketch::{CurveKind, Sketch};
use solidcraft_viewport::{Camera, Rgb, Scene, SceneAnalysis, SceneLine, SceneMesh};

use crate::{Result, Sel, Session};

/// Colours (shared with the GPU viewport).
pub mod colors {
    use solidcraft_viewport::Rgb;
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
    /// Isocurve Analysis curves: the light theme's accent.
    pub const ISOCURVE: Rgb = Rgb(38, 120, 218);
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
    pub const SECTION_CAP: Rgb = solidcraft_viewport::SECTION_CAP;
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
    /// The section hatch's other colours, after the cap's (`Scene::part_hatch`).
    pub hatch: [Rgb; 2],
    /// Isocurve Analysis: the faces' isoparametric curves.
    pub isocurve: Rgb,
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
        hatch: solidcraft_viewport::SECTION_HATCH,
        isocurve: colors::ISOCURVE,
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
/// hidden ones, or ones in hidden components). What Home, Fit and the orbit pivot frame. A model
/// that cannot be placed is the error.
pub fn bounds(s: &Session) -> Result<Aabb3> {
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
pub fn unmeshed_bounds(s: &Session) -> Result<Aabb3> {
    bounds_by(s, |body| body.cached_mesh().map(|m| m.bounds()).unwrap_or_else(|| body.outline_bounds()))
}

fn bounds_by(s: &Session, of: impl Fn(&solidcraft_doc::ModelBody) -> Aabb3) -> Result<Aabb3> {
    let st = s.world_state()?;
    let mut b = Aabb3::EMPTY;
    for body in st.bodies.iter().filter(|x| s.body_visible(&x.name)) {
        b = b.union(&of(body));
    }
    for ss in st.sketches.iter().filter(|x| sketch_shown(s, x.feature)) {
        // All four corners: on a tilted plane two opposite ones don't span the rectangle.
        if let Some((lo, hi)) = ss.sketch.bounds() {
            for p in [lo, hi, solidcore_record::Vec2::new(lo.x, hi.y), solidcore_record::Vec2::new(hi.x, lo.y)] {
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
    Ok(b)
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

/// The step of the grid's lines the viewport draws at this zoom (#9843).
///
/// Away from a cross-fade it is the step of the level the grid is drawn at. In a cross-fade that
/// level is the one with the bigger weight, and its lines are drawn as long as it has lines of
/// its own: the levels 2 and 5 do not nest, so each of them fades its own lines out over its
/// half of the cross-fade, and at the middle of it only the lines the two share (every 10 mm)
/// are drawn. The crossings the user can see are then those, and there is no crossing at 5 mm
/// to snap to.
fn grid_drawn_step(half_height: f64) -> f64 {
    let (minor, _) = grid_step(half_height);
    let Some(b) = grid_blend(half_height) else {
        return minor;
    };
    let (own_lo, own_hi) = b.own();
    // A nested level that still draws its own lines is drawn at its own step: the crossings the
    // user sees are the finer ones.
    if b.nested && own_lo > 0.0 {
        return b.lo;
    }
    // The level with the bigger weight is the one the grid is drawn at (`grid_step`).
    let (level, own) = if b.s >= 0.5 { (b.hi, own_hi) } else { (b.lo, own_lo) };
    if own > 0.0 { level } else { grid_shared_step(&b) }
}

/// The step of the lines two levels of a cross-fade share: the lines both of them draw.
///
/// The levels that nest (1 and 2, 5 and 10) share the lines of the coarser one. The levels that
/// do not nest (2 and 5) share only the lines every 10 mm, which is the finer level's major
/// step.
fn grid_shared_step(b: &GridBlend) -> f64 {
    if b.nested { b.hi } else { b.lo * 5.0 }
}

/// The grid a sketch's points snap to: the crossings of the lines the viewport draws, and the
/// finer lattice between them.
///
/// The crossings are the lines the viewport draws, so their step is the step of the lines that
/// are drawn ([`grid_drawn_step`]). The finer lattice is a tenth of the level the grid is drawn
/// at, so a value that is not on a crossing (an exact inch, say) can still be placed.
pub fn sketch_grid_step(half_height: f64) -> (f64, f64) {
    let (minor, _) = grid_step(half_height);
    (grid_drawn_step(half_height), minor / 10.0)
}

/// The crossing of the drawn grid's lines nearest `p`, when the pointer means that crossing:
/// the point the user can see and means (#7114).
///
/// The crossing takes the pointer when the pointer means it, and the finer lattice holds it
/// otherwise: the rule is in `sketch_grid_takes_crossing`.
pub fn sketch_grid_crossing(p: Vec2, minor: f64, fine: f64) -> Option<Vec2> {
    let crossing = Vec2::new((p.x / minor).round() * minor, (p.y / minor).round() * minor);
    let taken = sketch_grid_takes_crossing(crossing.dist(p), sketch_grid_lattice(p, fine).dist(p), fine);
    taken.then_some(crossing)
}

/// How close the pointer has to be to a point of the finer lattice for it to count as being on
/// it exactly: a thousandth of a fine step.
///
/// The pointer comes from the screen, so it is never exactly on a lattice point: the harness
/// leaves the pointer of the scenario `s12_pattern_bodies` at `14.000001516401388`, a millionth
/// and a half off. The tolerance is a thousandth of a fine step, so it swallows that noise and
/// the noise of any other view (both grow with the view), and it is still two hundred times
/// smaller than the quarter of a fine step the last clause measures.
const SKETCH_GRID_EXACT: f64 = 1e-3;

/// How many fine steps the crossing of the drawn grid may take the pointer (#9843).
///
/// The crossing is a point of the finer lattice too, so the pointer may land on it from two fine
/// steps away at most: the point does not move farther than that from where the pointer was.
const SKETCH_GRID_CROSSING_STEPS: f64 = 2.0;

/// Whether the crossing of the drawn grid's lines takes the pointer that is `d` away from it,
/// when the finer lattice's point is `l` away from it.
///
/// The rules, in order:
///
/// 1. **A pointer that is on a point of the finer lattice keeps that exact value.** The drawn
///    grid does not carry it off to the crossing beside it, so a value the user placed (`14`,
///    an exact radius) stays where it is.
/// 2. **The crossing takes the pointer when it is within a fine step of it.** That is the
///    crossing's own place on the finer lattice and the one right beside it, which is the case
///    the issue is about: the pointer is aimed at the crossing the user can see.
/// 3. **The crossing takes the pointer further out only when the finer lattice cannot hold
///    it, and only from two fine steps away.** A pointer that is more than a quarter of a fine
///    step away from any point of the lattice has no exact value to hold, so the crossing is
///    the better answer; but the crossing takes it only when it is no farther than two fine
///    steps, so that the point does not move more than two fine steps from where the pointer
///    was (#9843).
/// 4. Otherwise the finer lattice holds the pointer.
fn sketch_grid_takes_crossing(d: f64, l: f64, fine: f64) -> bool {
    // The exact value of a lattice point wins over the crossing beside it (rule 1).
    if l <= fine * SKETCH_GRID_EXACT && d > fine * SKETCH_GRID_EXACT {
        return false;
    }
    // The crossing takes the pointer within a fine step of it, or when the lattice cannot hold
    // it and the crossing is no farther than two fine steps (rules 2 and 3).
    d <= fine || (l > fine / 4.0 && d <= fine * SKETCH_GRID_CROSSING_STEPS)
}

/// The point of the finer lattice between the crossings nearest `p`.
pub fn sketch_grid_lattice(p: Vec2, fine: f64) -> Vec2 {
    Vec2::new(sketch_grid_lattice_axis(p.x, fine), sketch_grid_lattice_axis(p.y, fine))
}

/// The point of the finer lattice between the crossings nearest `v`.
pub fn sketch_grid_lattice_axis(v: f64, fine: f64) -> f64 {
    (v / fine).round() * fine
}

/// The point of the grid a sketch's point lands on: the crossing of the drawn grid's lines when
/// the pointer means one, and the finer lattice between the crossings otherwise (#7114).
pub fn sketch_grid_point(p: Vec2, minor: f64, fine: f64) -> Vec2 {
    match sketch_grid_crossing(p, minor, fine) {
        Some(crossing) => crossing,
        None => sketch_grid_lattice(p, fine),
    }
}

/// The free coordinate of a sketch's point that lines up with another point: the crossing of the
/// drawn grid's lines when the point can be on one, and on the finer lattice between the
/// crossings otherwise.
///
/// `free` is the coordinate the pointer gives and `fixed` the one the point lines up with, on the
/// other axis. The crossing has to run through `fixed`: a point that lines up with something off
/// the drawn grid is on no crossing of it, so there the finer lattice holds the point.
pub fn sketch_grid_free(free: f64, fixed: f64, minor: f64, fine: f64) -> f64 {
    let on_line = (fixed / minor).round() * minor;
    if (on_line - fixed).abs() > minor * 1e-6 {
        return sketch_grid_lattice_axis(free, fine);
    }
    let crossing = (free / minor).round() * minor;
    let lattice = sketch_grid_lattice_axis(free, fine);
    let taken = sketch_grid_takes_crossing((crossing - free).abs(), (lattice - free).abs(), fine);
    if taken { crossing } else { lattice }
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
        if c.spline_handle_of.is_some() {
            continue;
        }
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
    if c.spline_handle_of.is_some() {
        return false;
    }
    if c.link.is_some() && !sk.is_text_curve(i) {
        return !sk.view.hide_projected;
    }
    !(c.construction && sk.view.hide_construction)
}

/// One tangent guide of a selected fit spline: the guide runs from `mirror` through `fit` to
/// `handle`. `point` is the stored handle point, `None` while the spline only shows where its
/// handles start (they are stored by the first drag of one, #6502).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplineGuide {
    pub fit: Vec2,
    pub handle: Vec2,
    pub mirror: Vec2,
    pub point: Option<usize>,
}

/// The tangent guides spline `curve` shows while it is selected: one per fit point, from its
/// stored handles, or else from where [`Sketch::spline_handle_preview`] would start them. Empty
/// for a curve that is not a fit spline or cannot take handles.
pub fn spline_tangent_guides(sk: &Sketch, curve: usize) -> Vec<SplineGuide> {
    let Some(CurveKind::Spline { pts, control: false, handles, .. }) = sk.curves.get(curve).map(|c| &c.kind) else {
        return Vec::new();
    };
    let guide = |fit: Vec2, handle: Vec2, point| SplineGuide { fit, handle, mirror: fit * 2.0 - handle, point };
    if !handles.is_empty() {
        return pts.iter().zip(handles).filter_map(|(fit, handle)| Some(guide(sk.point(*fit)?, sk.point(*handle)?, Some(*handle)))).collect();
    }
    let Some(preview) = sk.spline_handle_preview(curve) else { return Vec::new() };
    pts.iter().zip(preview).filter_map(|(fit, handle)| Some(guide(sk.point(*fit)?, handle, None))).collect()
}

/// The full, two-sided display segment for a spline's tangent construction line.
pub fn spline_handle_display_line(sk: &Sketch, i: usize) -> Option<(Vec2, Vec2)> {
    let helper = sk.curves.get(i)?;
    let parent = helper.spline_handle_of.as_deref()?;
    let (CurveKind::Line { a, b }, parent_index) = (&helper.kind, sk.curve_index(parent)?) else {
        return None;
    };
    let CurveKind::Spline { pts, handles, .. } = &sk.curves.get(parent_index)?.kind else {
        return None;
    };
    let (fit_point, handle_point) = pts
        .iter()
        .zip(handles)
        .find_map(|(fit, handle)| ((*fit == *a && *handle == *b) || (*fit == *b && *handle == *a)).then_some((*fit, *handle)))?;
    let origin = sk.point(fit_point)?;
    let handle = sk.point(handle_point)?;
    let direction = handle - origin;
    Some((origin - direction, handle))
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
        for handle in sk.curves.iter().flat_map(|curve| match &curve.kind {
            CurveKind::Spline { handles, .. } => handles.as_slice(),
            _ => &[],
        }) {
            if let Some(point) = sk.points.get(*handle) {
                hidden.insert(point.id.as_str());
            }
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

/// The scene for a headless render, cut by the session's section. A model that cannot be placed
/// is the error.
pub fn scene(s: &Session, cam: &Camera) -> Result<Scene> {
    scene_with_section(s, cam, s.section)
}

/// The scene for a headless render, cut by `section` (origin, normal) instead of the session's:
/// the app passes the cut its viewport shows (an open Section Analysis dialog's preview, a
/// sketch's Slice), so model renders match the window.
pub fn scene_with_section(s: &Session, cam: &Camera, section: Option<(Vec3, Vec3)>) -> Result<Scene> {
    scene_with_guides(s, cam, section, Guides::default())
}

/// The plane of the sketch that is open, if any: it always shows a grid, whatever the Grid switch
/// says (outside a sketch the grid is the user's choice).
pub fn open_sketch_plane(s: &Session) -> Result<Option<Plane>> {
    let Some(id) = s.active_sketch else { return Ok(None) };
    Ok(s.world_state()?.sketch(id).map(|ss| ss.plane))
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
pub fn scene_with_guides(s: &Session, cam: &Camera, section: Option<(Vec3, Vec3)>, guides: Guides) -> Result<Scene> {
    scene_with_palette_and_guides(s, cam, section, world_curve_parts, &Palette::DEFAULT, guides)
}

/// A headless scene using supplied display paths for its visible sketch curves.
/// Callers can use the same pixel sampler as a GPU viewport without changing the
/// body, grid, appearance, section or sketch styling policies of [`scene`].
pub fn scene_with_sketch_sampler(s: &Session, cam: &Camera, sampler: impl FnMut(&Sketch, usize, &Plane) -> Vec<Vec<Vec3>>) -> Result<Scene> {
    scene_with_section_and_sketch_sampler(s, cam, s.section, sampler)
}

/// A headless scene with an explicit section and supplied sketch display paths.
pub fn scene_with_section_and_sketch_sampler(
    s: &Session,
    cam: &Camera,
    section: Option<(Vec3, Vec3)>,
    sampler: impl FnMut(&Sketch, usize, &Plane) -> Vec<Vec<Vec3>>,
) -> Result<Scene> {
    scene_with_palette(s, cam, section, sampler, &Palette::DEFAULT)
}

/// [`scene_with_section_and_sketch_sampler`] in the colours of `p` (the app's theme).
pub fn scene_with_palette(
    s: &Session,
    cam: &Camera,
    section: Option<(Vec3, Vec3)>,
    sampler: impl FnMut(&Sketch, usize, &Plane) -> Vec<Vec<Vec3>>,
    p: &Palette,
) -> Result<Scene> {
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
) -> Result<Scene> {
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
) -> crate::Result<Scene> {
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
) -> Result<Scene> {
    let st = s.world_state()?;
    let section = section.filter(|(o, n)| o.is_finite() && n.is_finite() && n.len() > 1e-9);
    let mut sc = Scene {
        background: Some(p.background),
        clip: section.map(|(o, n)| (n, n.dot(o))),
        cap: Some(p.cap),
        plain_cap: !s.section_hatch,
        part_hatch: Some([p.cap, p.hatch[0], p.hatch[1]]),
        analysis: s.analysis.map(|a| SceneAnalysis { mode: a.mode(), colors: p.analysis }),
        ..Default::default()
    };
    let b = bounds(s)?;
    sc.radius = (b.diagonal() * 0.5).max(cam.half_height()).max(10.0) + b.center().dist(cam.target);
    // Grid on the ground plane (XY with Z up, XZ with Y up), drawn under the model as in the viewport.
    if guides.grid {
        let major = grid_step(cam.half_height()).1;
        // Headless pictures keep the grid on the ground, the viewport's default; an open sketch
        // has its own plane's grid (world coordinates, no camera frame).
        let (c, u, v, slant, w): (Vec3, Vec3, Vec3, f64, Box<dyn Fn(Vec3) -> Vec3>) = if let Some(pl) = open_sketch_plane(s)? {
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
        let origin = open_sketch_plane(s)?.map_or(Vec3::ZERO, |pl| pl.origin);
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
    // Where the section cuts each closed solid body, by its mesh in `sc.meshes`: their hatches.
    let mut cuts: Vec<(usize, u64, Vec<(Vec3, Vec3)>)> = Vec::new();
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
        let cut_solid = section.is_some() && body.is_closed() && opacity >= solidcraft_viewport::SEE_THROUGH;
        sc.meshes.push(SceneMesh { mesh: Arc::clone(&mesh), color, face_colors, opacity, closed: body.is_closed(), hatch: Default::default() });
        let at = sc.meshes.len() - 1;
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
            let segments = section_segments(&mesh, o, n);
            for (a, b) in &segments {
                sc.lines.push(SceneLine { points: vec![*a, *b], color: p.edge, alpha: 1.0, width: 1.6, on_top: false });
            }
            if cut_solid && !segments.is_empty() {
                cuts.push((at, solidcraft_viewport::hatch::seed(&body.name), segments));
            }
        }
    }
    if let Some((_, n)) = section {
        let parts: Vec<solidcraft_viewport::hatch::CutPart> =
            cuts.iter().map(|(_, seed, segments)| solidcraft_viewport::hatch::CutPart { seed: *seed, segments }).collect();
        for ((at, _, _), hatch) in cuts.iter().zip(solidcraft_viewport::hatch::hatches(&parts, n)) {
            if let Some(m) = sc.meshes.get_mut(*at) {
                m.hatch = hatch;
            }
        }
    }
    // Isocurve Analysis: the isoparametric curves of each face it shows, on the faces.
    for line in s.isocurve_lines().iter().flat_map(|lines| lines.iter()) {
        sc.lines.push(SceneLine { points: line.points.clone(), color: p.isocurve, alpha: 1.0, width: 1.4, on_top: false });
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
    Ok(sc)
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
/// Sketches folder or group (even one shown on its own), and not when hidden with its own eye.
/// The sketch immediately before a rollback marker is shown even when a feature uses it, so a
/// step on the History bar shows what it adds (#5202); other sketches used by a feature are
/// shown only when shown on their own. The user's own hide wins over the marker (#12125): the
/// browser shows the eye off, and hiding the sketch again must take it away.
pub fn sketch_shown(s: &Session, id: u64) -> bool {
    let v = &s.visibility;
    if s.active_sketch == Some(id) || s.feature_edit_sketch == Some(id) {
        return true;
    }
    if !s.feature_placement_visible(id) || s.feature_folder_hidden(id, "sketches") || v.hidden_sketches.contains(&id) {
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
    v.shown_sketches.contains(&id) || !sketch_consumed(s, id)
}

/// Is a sketch used by a later feature? A sheet metal base or contour flange, or a perforation's
/// outline, uses its sketch as an Extrude does: shown, its lines lie on the sheet's edges and
/// faces and take the clicks meant for them (#6804). So does a Hole at sketch points: shown, its
/// points are drawn in the holes (#9082). A Single hole snapped to one point (#2110) does not: it
/// was placed on a face, not built from the sketch, and hiding the sketch would take its other
/// points from the next Single hole's snap.
pub fn sketch_consumed(s: &Session, id: u64) -> bool {
    if let Ok(mut cache) = s.consumed_sketches_cache.lock() {
        if cache.as_ref().is_none_or(|c| !std::ptr::eq(c.doc.as_ptr(), Arc::as_ptr(&s.doc))) {
            *cache = Some(ConsumedSketches { doc: Arc::downgrade(&s.doc), ids: consumed_sketches(&s.doc).collect() });
        }
        return cache.as_ref().is_some_and(|c| c.ids.contains(&id));
    }
    // The lock is poisoned: answer from the features themselves.
    consumed_sketches(&s.doc).any(|sketch| sketch == id)
}

/// The sketches [`sketch_consumed`] answers for, read from one design. The design is held
/// weakly: its allocation is not handed to another design while the set lives, and an edit in
/// place (`Arc::make_mut`) moves it to an allocation of its own, so a set is never asked about
/// a design it was not read from.
pub(crate) struct ConsumedSketches {
    doc: std::sync::Weak<solidcraft_doc::Document>,
    ids: std::collections::HashSet<u64>,
}

/// The sketches the features of a design use (see [`sketch_consumed`]), once per use.
fn consumed_sketches(doc: &solidcraft_doc::Document) -> impl Iterator<Item = u64> + '_ {
    use solidcraft_doc::{FeatureKind, SketchPoints};
    sketch_work::add(doc.features.len() as u64);
    doc.features.iter().filter_map(|f| match &f.kind {
        FeatureKind::Extrude { sketch, .. }
        | FeatureKind::Revolve { sketch, .. }
        | FeatureKind::SheetBase { sketch, .. }
        | FeatureKind::SheetContour { sketch, .. }
        | FeatureKind::Hole { points: Some(SketchPoints { sketch, single: false, .. }), .. }
        | FeatureKind::SheetPerforate { sketch: Some(sketch), .. } => Some(sketch.0),
        _ => None,
    })
}

/// How many features [`sketch_consumed`] walked, for the tests that hold it to one walk per
/// design rather than one per question.
pub(crate) mod sketch_work {
    #[cfg(test)]
    thread_local! {
        static WALKED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    /// The features the answers of this thread walked since the last call.
    #[cfg(test)]
    pub(crate) fn take() -> u64 {
        WALKED.with(|walked| walked.replace(0))
    }

    pub(crate) fn add(walked: u64) {
        #[cfg(test)]
        WALKED.with(|count| count.set(count.get() + walked));
        #[cfg(not(test))]
        let _ = walked;
    }
}

/// Camera framing the model (iso view).
pub fn home_camera(s: &Session) -> Result<Camera> {
    let mut c = Camera::default();
    let b = bounds(s)?;
    if !b.is_empty() {
        c.fit(&solidcraft_render::view_bounds(&b));
    } else {
        c.distance = 150.0;
    }
    Ok(c)
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

/// The planar body face each construction plane was made from, as it is now: (plane name, body
/// name, the face's indices on that body), for a plane of the design (not a component's) whose
/// definition reaches an attached face: an Offset Plane on a face, and planes offset, turned,
/// flipped or moved through a point from it. The viewport draws such a plane over its face, which
/// its origin, the plane's point nearest the world origin, can be far from (#4345). A plane on a
/// face found again only by its picked point or name, or whose face is gone, has none.
pub fn construction_plane_faces(s: &Session) -> Vec<(String, String, Vec<usize>)> {
    let st = s.model.state();
    s.doc
        .features
        .iter()
        .filter(|f| !f.suppressed && f.component == 0)
        .filter_map(|f| {
            let solidcraft_doc::FeatureKind::ConstructionPlane { plane } = &f.kind else { return None };
            let anchor = plane_face_anchor(&s.doc, plane, 0).filter(|a| a.path.is_empty())?;
            let body = st.body(&anchor.target.body)?;
            let (faces, _) = s.doc.anchored_face_indices(&st, body, &anchor.target).ok()?;
            (!faces.is_empty()).then(|| (f.name.clone(), anchor.target.body.clone(), faces))
        })
        .collect()
}

/// The face anchor a plane's definition reaches: its own, its base's (offset, turned, flipped,
/// moved through a point) or, through a construction plane it is built on, that plane's.
fn plane_face_anchor<'a>(
    doc: &'a solidcraft_doc::Document,
    plane: &'a solidcraft_doc::PlaneRef,
    depth: usize,
) -> Option<&'a solidcraft_doc::FaceAnchor> {
    use solidcraft_doc::PlaneRef;
    match plane {
        PlaneRef::Face { anchor, .. } => anchor.as_deref(),
        PlaneRef::Offset { base, .. } | PlaneRef::ThroughPoint { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => {
            plane_face_anchor(doc, base, depth + 1)
        }
        PlaneRef::Construction { feature } if depth < 32 => match &doc.feature(feature.0)?.kind {
            solidcraft_doc::FeatureKind::ConstructionPlane { plane } => plane_face_anchor(doc, plane, depth + 1),
            _ => None,
        },
        _ => None,
    }
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
        let cam = home_camera(&s).unwrap();
        let sc = scene(&s, &cam).unwrap();
        assert!(sc.meshes.iter().any(|m| m.color == Rgb(220, 60, 40)));
        assert!(sc.meshes.iter().any(|m| m.face_colors.iter().any(|(_, c)| *c == Rgb(40, 170, 80))));
    }

    /// A headless picture draws each body's own display mesh, the allocation the model holds:
    /// the renderer's mesh is the model's type, so nothing is copied or converted on the way.
    #[test]
    fn snapshot_scene_shares_the_bodies_meshes() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let sc = scene(&s, &home_camera(&s).unwrap()).unwrap();
        let state = s.model.state();
        let body = state.bodies.first().unwrap().mesh();
        assert!(sc.meshes.iter().any(|m| Arc::ptr_eq(&m.mesh, &body)));
    }

    /// A render can leave out the ground grid and the origin's axes (a box straddling Z 0 is cut
    /// through by the grid), each on its own; the body's own lines stay.
    #[test]
    fn guides_leave_the_grid_and_the_axes_out() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20, "corner": [10, -15, -10]})).unwrap();
        let cam = home_camera(&s).unwrap();
        let grid = [colors::GRID, colors::GRID_MAJOR];
        let axes = [colors::AXIS_X, colors::AXIS_Y, colors::AXIS_Z];
        let count = |sc: &Scene, colours: &[Rgb]| sc.grid.iter().chain(&sc.lines).filter(|l| colours.contains(&l.color)).count();
        let all = scene(&s, &cam).unwrap();
        let (grid_lines, edges) = (count(&all, &grid), count(&all, &[colors::EDGE]));
        assert!(grid_lines > 20 && edges >= 12, "{grid_lines} grid lines, {edges} edges");
        assert_eq!(count(&all, &axes), 3);
        assert_eq!(all.lines.len(), scene_with_guides(&s, &cam, s.section, Guides::default()).unwrap().lines.len());
        let render = |g: Guides| scene_with_guides(&s, &cam, s.section, g).unwrap();
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
        let cam = home_camera(&s).unwrap();
        let axes = [colors::AXIS_X, colors::AXIS_Y, colors::AXIS_Z];
        let shown = |s: &Session| -> Vec<Rgb> {
            let sc = scene(s, &cam).unwrap();
            axes.into_iter().filter(|c| sc.lines.iter().any(|l| l.color == *c)).collect()
        };
        assert_eq!(shown(&s), axes);
        s.execute("browser.visibility", &json!({"items": ["Y"], "visible": false})).unwrap();
        assert_eq!(shown(&s), [colors::AXIS_X, colors::AXIS_Z]);
        s.execute("browser.visibility", &json!({"folder": "origin", "visible": false})).unwrap();
        assert!(shown(&s).is_empty());
        let sc = scene_with_guides(&s, &cam, None, Guides { grid: true, axes: true }).unwrap();
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
        let cam = home_camera(&s).unwrap();
        assert!(scene(&s, &cam).unwrap().clip.is_none());
        s.execute("inspect.section", &json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
        let sc = scene(&s, &cam).unwrap();
        let (n, d) = sc.clip.unwrap();
        assert!((n - Vec3::new(0.0, -1.0, 0.0)).len() < 1e-12 && (d + 15.0).abs() < 1e-12, "{n:?} {d}");
        assert_eq!(sc.cap, Some(colors::SECTION_CAP));
        assert!(!section_segments(&s.world_state().unwrap().bodies[0].mesh(), Vec3::new(0.0, 15.0, 0.0), -Vec3::Y).is_empty());
    }

    /// The surface analysis shades the headless scene too, in the palette's colours.
    #[test]
    fn the_surface_analysis_shades_the_scene() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let cam = home_camera(&s).unwrap();
        assert!(scene(&s, &cam).unwrap().analysis.is_none());
        s.analysis = Some(crate::SurfaceAnalysis::Curvature { radius: 5.0 });
        let a = scene(&s, &cam).unwrap().analysis.unwrap();
        assert_eq!((a.mode, a.colors), (solidcraft_viewport::AnalysisMode::Curvature { radius: 5.0 }, Palette::DEFAULT.analysis));
    }

    /// The CPU rasterizer (the last tier of headless pictures, `SOLIDCRAFT_GPU=cpu` or no adapter
    /// under `auto`): curved surfaces get a dark silhouette line where they turn away.
    #[test]
    fn cpu_pictures_draw_silhouettes_on_curved_surfaces() {
        let mut s = Session::default();
        s.execute("solid.sphere", &json!({"diameter": 20})).unwrap();
        let cam = home_camera(&s).unwrap();
        let img = solidcraft_viewport::render(&scene(&s, &cam).unwrap(), &cam, 400, 400);
        let dark = img.rgba.chunks(4).filter(|p| p[0] < 70 && p[1] < 70 && p[2] < 75).count();
        assert!(dark > 40, "silhouette pixels: {dark}");
    }

    /// The CPU rasterizer: a section shows; the cut part is gone and the cut face is drawn.
    #[test]
    fn cpu_pictures_show_the_section() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let cam = home_camera(&s).unwrap();
        let whole = solidcraft_viewport::render(&scene(&s, &cam).unwrap(), &cam, 200, 140).rgba;
        s.execute("inspect.section", &json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
        let sc = scene(&s, &cam).unwrap();
        assert!(sc.clip.is_some());
        let cut = solidcraft_viewport::render(&sc, &cam, 200, 140).rgba;
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
            let mut cam = home_camera(&s).unwrap();
            cam.fov = fov;
            let mut sc = scene(&s, &cam).unwrap();
            // The cap in the cap colour: a part's hatch colour is the one its name picks.
            sc.part_hatch = None;
            assert!(!section_segments(&s.world_state().unwrap().bodies[0].mesh(), Vec3::new(0.0, 15.0, 0.0), -Vec3::Y).is_empty());
            // A marker inside the body, behind the cut face.
            let red = Rgb(255, 0, 0);
            sc.lines.push(SceneLine {
                points: vec![Vec3::new(5.0, 25.0, 10.0), Vec3::new(35.0, 25.0, 10.0)],
                color: red,
                alpha: 1.0,
                width: 3.0,
                on_top: false,
            });
            let img = solidcraft_viewport::render(&sc, &cam, 300, 220);
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
        let cam = home_camera(&s).unwrap();
        let mut picture = |hatch: bool| {
            let plane = json!({"origin": [0, 15, 0], "normal": [0, -1, 0]});
            s.execute("inspect.section", &json!({"plane": plane, "hatch": hatch})).unwrap();
            solidcraft_viewport::render(&scene(&s, &cam).unwrap(), &cam, 300, 220).rgba
        };
        // Pixels in the body's exact colour: only a plain cut has them (lit faces are shaded).
        let solid = |img: &[u8]| img.chunks(4).filter(|p| p[..3] == [40, 160, 90]).count();
        let (hatched, plain) = (picture(true), picture(false));
        assert!(solid(&plain) > 500, "plain cut: {} pixels in the body's colour", solid(&plain));
        assert_eq!(solid(&hatched), 0, "the hatched cut is tinted");
        let shades = |img: &[u8]| img.chunks(4).map(|p| [p[0], p[1], p[2]]).collect::<std::collections::BTreeSet<_>>().len();
        assert!(shades(&hatched) > shades(&plain), "the hatch adds its stripes' shades");
    }

    /// A post standing on a plate, cut through: the post's cut is its own (blue) cap down to the
    /// plate, whichever body came first; the plate's top (the same plane as the post's bottom,
    /// seen through the cut) does not show through it (the viewport: `solidcraft-gpu`'s depth tests).
    #[test]
    fn cpu_picture_a_body_standing_on_another_keeps_its_own_cap() {
        let plate = ("solid.box", json!({"length": 80, "width": 50, "height": 10, "body_name": "Plate"}));
        let post = ("solid.cylinder", json!({"radius": 12, "height": 40, "base": [20, 25, 10], "body_name": "Post"}));
        for post_first in [false, true] {
            let mut s = Session::default();
            for (c, p) in if post_first { [&post, &plate] } else { [&plate, &post] } {
                s.execute(c, p).unwrap();
            }
            s.execute("appearance.assign", &json!({"bodies": ["Plate"], "appearance": "ABS - Red"})).unwrap();
            s.execute("appearance.assign", &json!({"bodies": ["Post"], "appearance": "ABS - Blue"})).unwrap();
            s.execute("inspect.section", &json!({"plane": {"origin": [0, 25, 0], "normal": [0, -1, 0]}})).unwrap();
            // The home view, and one looking into the cut at an angle (#4622 review).
            let oblique = |c: &mut Camera| (c.target, c.yaw, c.pitch, c.distance) = (Vec3::new(30.0, 25.0, 12.0), 0.25, 0.4, 90.0);
            for (fov, angled) in [(0.0, false), (0.6, false), (0.0, true), (0.6, true)] {
                let mut cam = home_camera(&s).unwrap();
                cam.fov = fov;
                if angled {
                    oblique(&mut cam);
                }
                let sc = scene(&s, &cam).unwrap();
                let img = solidcraft_viewport::render(&sc, &cam, 400, 400);
                let vp = cam.view_proj(1.0, sc.radius);
                let mut wrong = Vec::new();
                for x in (10..=30).map(f64::from) {
                    for z in [11.0, 12.0, 13.0, 14.0] {
                        let q = vp.apply(Vec3::new(x, 25.0, z));
                        let (px, py) = (((q[0] / q[3] + 1.0) * 200.0) as usize, ((1.0 - q[1] / q[3]) * 200.0) as usize);
                        let p = &img.rgba[(py * 400 + px) * 4..][..3];
                        if i16::from(p[2]) <= i16::from(p[0]) + 25 {
                            wrong.push(((x, z), p.to_vec()));
                        }
                    }
                }
                assert!(wrong.is_empty(), "fov {fov}, angled {angled}, post first {post_first}: not the post's cap at {wrong:?}");
            }
        }
    }

    /// The viewport asks whether each sketch is used several times a frame: the features are
    /// walked once per design, not once per sketch asked about, and the answers follow every
    /// change of the design, a direct edit of it included.
    #[test]
    fn consumed_sketches_are_worked_out_once_per_design() {
        let mut s = Session::default();
        let mut sketches = Vec::new();
        for i in 0..4 {
            let at = 20 * i;
            sketches.push(s.execute("sketch.create", &json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap());
            s.execute("sketch.rectangle.two_point", &json!({"p0": [at, 0], "p1": [at + 10, 10]})).unwrap();
            s.execute("sketch.finish", &json!({})).unwrap();
        }
        s.execute("solid.extrude", &json!({"sketch": sketches[0], "distance": 5})).unwrap();
        s.execute("solid.extrude", &json!({"sketch": sketches[2], "distance": 5, "operation": "new"})).unwrap();
        let shown = |s: &Session| sketches.iter().map(|id| sketch_shown(s, *id)).collect::<Vec<_>>();
        sketch_work::take();
        for _ in 0..10 {
            assert_eq!(shown(&s), [false, true, false, true]);
        }
        assert_eq!(sketch_work::take(), s.doc.features.len() as u64, "one walk of the features for 40 questions");
        s.execute("solid.extrude", &json!({"sketch": sketches[1], "distance": 5, "operation": "new"})).unwrap();
        assert_eq!(shown(&s), [false, false, false, true], "an Extrude added uses its sketch");
        let at = s.doc.features.iter().position(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Extrude { .. })).unwrap();
        Arc::make_mut(&mut s.doc).features.remove(at);
        assert_eq!(shown(&s), [true, false, false, true], "a design edited in place is read again");
        // `doc_mut` edits in place (the crash journal follows the document's address, #7502):
        // the set held on the design does not move it.
        let address = Arc::as_ptr(&s.doc);
        s.doc_mut().features.retain(|f| !matches!(f.kind, solidcraft_doc::FeatureKind::Extrude { .. }));
        assert_eq!(Arc::as_ptr(&s.doc), address, "doc_mut still edits in place");
        assert_eq!(shown(&s), [true; 4], "an edit through doc_mut is read again");
    }

    /// The crossing of the drawn grid's lines takes the pointer when the pointer means it, and
    /// the finer lattice holds the pointer otherwise (#7114).
    ///
    /// The numbers are the ones the harness's camera gives: a half height of 80 makes the
    /// drawn grid's step 5 and the finer lattice's step 0.5.
    ///
    /// A pointer that is exactly on a point of the finer lattice holds that exact value: the
    /// drawn grid does not carry it off to the crossing beside it. That is what keeps the
    /// radius of the circle of the scenario `s12_pattern_bodies` at 4: the click at `x=14`
    /// lands on the lattice's own point 14 and not on the crossing 15 beside it.
    ///
    /// A pointer that is not on a lattice point but is within a fine step of a crossing is
    /// aimed at that crossing: that is the case the issue is about, and the test of the issue
    /// is the pointer at `20.3125`, a sixteenth of a cell off the crossing 20.
    #[test]
    fn the_crossing_of_the_drawn_grid_takes_the_pointer_that_means_it() {
        let point = |p: Vec2, minor: f64| sketch_grid_point(p, minor, minor / 10.0);
        let near = |a: Vec2, b: Vec2| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9;
        // The drawn grid's step 5 and the finer lattice's step 0.5.
        // A pointer off a crossing but within a fine step of it lands on the crossing: the
        // pointer is at 20.3125, the crossing is at 20.0, and the distance is 0.3125.
        assert!(near(point(Vec2::new(20.3125, 20.0), 5.0), Vec2::new(20.0, 20.0)));
        // A pointer exactly on a point of the finer lattice keeps that exact value: the
        // pointer at 10.5 is 0.5 from the crossing 10.0, and it stays.
        assert!(near(point(Vec2::new(10.5, 20.0), 5.0), Vec2::new(10.5, 20.0)));
        assert!(near(point(Vec2::new(14.5, 20.0), 5.0), Vec2::new(14.5, 20.0)));
        assert!(near(point(Vec2::new(9.5, 20.0), 5.0), Vec2::new(9.5, 20.0)));
        // The radius of the circle of the scenario s12_pattern_bodies: the click at 14 is on
        // the lattice's own point and stays, a whole fine step from the crossing 15.
        assert!(near(point(Vec2::new(14.0, 10.0), 5.0), Vec2::new(14.0, 10.0)));
        // A pointer on a crossing is on the crossing and on the lattice at once.
        assert!(near(point(Vec2::new(10.0, 20.0), 5.0), Vec2::new(10.0, 20.0)));
        // With small cells the lattice holds more of the pointer: at a step of 1 the pointer
        // at 20.5 is 0.5 from the crossing 21.0, half a cell, and the lattice holds it.
        assert!(near(point(Vec2::new(20.5, 20.0), 1.0), Vec2::new(20.5, 20.0)));
        // But the pointer at 20.05 is 0.05 from the crossing 20.0, within a fine step of
        // 0.1, so the crossing takes it.
        assert!(near(point(Vec2::new(20.05, 20.0), 1.0), Vec2::new(20.0, 20.0)));
        assert!(near(point(Vec2::new(20.0, 20.0), 1.0), Vec2::new(20.0, 20.0)));
        // The free coordinate of a point that lines up with another one follows the same rule.
        let free = |v: f64, fixed: f64, minor: f64| sketch_grid_free(v, fixed, minor, minor / 10.0);
        assert!((free(20.3125, 20.0, 5.0) - 20.0).abs() < 1e-9);
        assert!((free(10.5, 20.0, 5.0) - 10.5).abs() < 1e-9);
        assert!((free(14.0, 10.0, 5.0) - 14.0).abs() < 1e-9);
    }

    /// A point snaps only to a crossing the grid draws (#7114).
    ///
    /// At the middle of the cross-fade between the levels 2 and 5 the grid draws only the lines
    /// the two levels share, every 10 mm: the level of 5 mm is not drawn there, so there is no
    /// crossing at 5 mm for the point to land on. The step the sketch snaps to is the step of
    /// the lines that are drawn, not the step of the level that is fading out.
    #[test]
    fn a_point_snaps_only_to_a_crossing_the_grid_draws() {
        // The lines the viewport draws at that zoom: the ones the two levels share.
        let drawn: Vec<f64> = grid_lines(50.0, 0.0, 40.0, 8).iter().map(|l| l.offset).collect();
        assert!(drawn.iter().all(|o| (o / 10.0).round() * 10.0 == *o), "the lines drawn: {drawn:?}");
        assert!(!drawn.iter().any(|o| (*o - 5.0).abs() < 1e-9), "there is no line at 5 mm");
        // The step the sketch snaps to: the one of the lines that are drawn.
        let (minor, fine) = sketch_grid_step(50.0);
        assert_eq!(minor, 10.0, "the step of the lines that are drawn");
        assert_eq!(fine, 0.5, "the finer lattice's step");
        // A pointer at 5.3125 is not near a crossing of the drawn grid: it does not land on the
        // crossing 5 that is not drawn.
        let p = sketch_grid_point(Vec2::new(5.3125, 10.0), minor, fine);
        assert!(p.dist(Vec2::new(5.0, 10.0)) > 1e-9, "the point landed on the crossing 5 that is not drawn: {p:?}");
    }

    /// The finer lattice holds the point when the grid refuses the crossing (#7114).
    ///
    /// The grid's helpers never give the pointer back: when there is no crossing to take it, the
    /// point lands on the finer lattice, which holds the value the user placed.
    #[test]
    fn the_fine_lattice_holds_the_point_when_the_grid_refuses_the_crossing() {
        let (minor, fine) = sketch_grid_step(50.0);
        // At that zoom the grid draws the lines every 10 mm only: there is no crossing at 5 mm,
        // so the point lands on the lattice's own point 5.5.
        let p = sketch_grid_point(Vec2::new(5.3125, 10.0), minor, fine);
        assert!(p.dist(Vec2::new(5.5, 10.0)) < 1e-9, "the point lands at {p:?}");
        // The pointer itself is never the answer.
        assert!(p.dist(Vec2::new(5.3125, 10.0)) > 1e-9, "the point landed on the pointer");
    }

    /// The crossing of the drawn grid does not take a pointer that is more than two fine steps
    /// away from it (#7114).
    ///
    /// With the drawn step 5 and the fine step 0.5, a pointer at 21.2 is 1.2 mm from the
    /// crossing 20 (two and a half fine steps) and 0.2 from the lattice's own point 21: the
    /// lattice holds the pointer, so the point does not move more than two fine steps away from
    /// where it was.
    #[test]
    fn the_crossing_of_the_grid_does_not_take_a_pointer_more_than_two_fine_steps_away() {
        let p = sketch_grid_point(Vec2::new(21.2, 20.0), 5.0, 0.5);
        assert!(p.dist(Vec2::new(21.0, 20.0)) < 1e-9, "the point lands at {p:?}");
    }

    /// The nested cross-fade: while the finer level still draws its own lines, the crossings the
    /// user sees are the finer ones, so a point snaps to them (#9843).
    #[test]
    fn a_nested_cross_fade_snaps_to_the_visible_crossing() {
        let (minor, fine) = sketch_grid_step(110.0);
        assert_eq!(minor, 5.0, "the finer level's lines are still drawn");
        assert_eq!(sketch_grid_point(Vec2::new(5.7, 10.0), minor, fine), Vec2::new(5.0, 10.0));
    }
}

#[cfg(test)]
#[path = "view_display_tests.rs"]
mod display_tests;

#[cfg(test)]
#[path = "view_palette_tests.rs"]
mod palette_tests;
