//! Collision checks for multi-axis toolpaths: the tool's shank and holder against the part and
//! fixtures, and tilting the tool away where they would touch.
//!
//! The checks work on any [`Toolpath5`], whatever the tool axis: 3-axis paths turned into one,
//! indexed and rotary ones, and simultaneous 5-axis ones. Each cutting move is sampled finely
//! enough that neither the tip nor any point of the tool up to the top of what is checked moves
//! more than the sample step between samples: the tip on its straight line, the axis turning
//! evenly between the two directions, as the move is cut. [`gouges`] and [`holder_collisions`]
//! do not sample rapid moves: they are the links' business, planned clear of the part.
//! [`tilt_away`], which changes the axis on the rapids as well, checks every move it returns.
//!
//! A gouge is the cutter deeper in the surface than the tolerance: the cutter shrunk by the
//! tolerance must not meet it. A tool that only touches a wall beside it, or is within the
//! tolerance of it sideways, does not gouge it. How deep a gouge is, is the deepest point of
//! the surface inside the cutter measured to the cutter's own surface ([`depth_at`]), not how
//! far the cutter would have to back out along its axis, which on a steep wall is far more.

use std::cell::Cell;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::cutter::Cutter;
use crate::dropcutter::{drop_along, drop_along_above};
use crate::multiaxis::{Move5, Pose, Toolpath5, slerp, verify5};
use crate::path::MAX_MOVES;
use crate::surface::{Surface, capsule_meets_box, nearest_on_triangle, nearest_segments, ray_triangle, segment_triangle_distance};
use crate::tool::{Envelope, Tool};
use crate::{CamError, Result, invalid};

/// Most samples one check takes along a toolpath.
const MAX_SAMPLES: usize = 20_000_000;
/// Smallest tilt increment accepted (radians).
const MIN_TILT_STEP: f64 = 1e-4;

/// Where a toolpath makes the tool touch what it must not.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    /// The move (index into the path's moves) along which it happens.
    pub index: usize,
    /// The worst pose found along that move.
    pub pose: Pose,
    /// Which of [`Tool::envelopes`] touches; `None` for the cutter itself (a gouge).
    pub envelope: Option<usize>,
    /// For a gouge, how deep the cutter is in the surface there (mm, see [`depth_at`]). For an
    /// envelope, the gap between its cylinder and the nearest triangle (mm), below the clearance
    /// asked for; 0 or less where they touch or cross.
    pub distance: f64,
}

fn check_step(step: f64) -> Result<()> {
    if !(step > 0.0) || !(step <= 1.0e6) {
        return invalid("the sample step must be above 0 and at most 1000 m");
    }
    Ok(())
}

fn check_path(path: &Toolpath5) -> Result<()> {
    if path.moves.len() > MAX_MOVES {
        return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
    }
    for (i, m) in path.moves.iter().enumerate() {
        if m.target().is_some_and(|p| !p.is_valid()) {
            return invalid(format!("move {i}: a pose is not finite or its tool axis is not a unit direction"));
        }
    }
    Ok(())
}

/// Number of samples along a move from `from` to `to` for points up to `reach` up the tool.
fn samples_along(from: &Pose, to: &Pose, reach: f64, step: f64) -> Result<usize> {
    let len = from.tip.dist(to.tip) + from.axis_angle(to) * reach;
    let n = (len / step - 1e-9).ceil().max(1.0);
    if !(n <= MAX_SAMPLES as f64) {
        return Err(CamError::TooLarge(format!("more than {MAX_SAMPLES} samples along one move")));
    }
    Ok(n as usize)
}

/// Which moves of a toolpath a check samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Moves {
    /// The cutting moves.
    Cutting,
    /// Every move.
    All,
}

/// What a gouge is: the cutter shrunk by the tolerance, which the surface meets only where the
/// cutter is deeper in it than that.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GougeTest {
    /// The cutter shrunk all round, its tip `lift` up the axis.
    inner: Cutter,
    lift: f64,
    tol: f64,
}

impl GougeTest {
    pub(crate) fn new(cutter: &Cutter, tol: f64) -> Result<GougeTest> {
        cutter.validate()?;
        if !(tol >= 0.0) || !(tol <= 1.0e3) {
            return invalid("the gouge tolerance must be from 0 to 1000 mm");
        }
        // Sideways by at most half the radius, so that a tolerance as large as the cutter still
        // finds it deep in the surface along its axis; the shrunk cutter, raised by the whole
        // tolerance (a tapered one's side moved in by it), stays inside the cutter.
        let (inner, lift) = cutter.shrunk(tol)?;
        Ok(GougeTest { inner, lift, tol })
    }

    /// How far the cutter at `pose` is in the surface, where deeper than the tolerance: how much
    /// farther out along the axis it would have to be for the shrunk cutter to clear it, plus
    /// the tolerance (a quick test, not the depth: see [`depth_at`]).
    pub(crate) fn depth(&self, surface: &Surface, pose: &Pose) -> Result<Option<f64>> {
        Ok(drop_along_above(surface, &self.inner, pose.up(self.lift), pose.axis, 0.0)?.map(|d| d + self.tol))
    }
}

/// Halvings that find a depth: to a billionth of the cutter's radius.
const DEPTH_HALVINGS: usize = 30;
/// Golden-section steps that find the deepest point along a move near its deepest sample.
const DEEPEST_STEPS: usize = 24;
/// Most samples of one move measured once the deepest is buried deeper than the radius.
const MAX_BURIED_MEASURES: usize = 64;
/// Where along each stretch between two samples of a move the cutter is tested besides its
/// ends: half way and near the thirds (see [`crate::surface_ops`]), so a bulge of the surface
/// between two samples is met near its top.
const BETWEEN: [f64; 3] = [0.381_966_011_250_105_1, 0.5, 0.618_033_988_749_894_9];

/// How deep the cutter (endless above its end) at `pose` is in the surface (mm; 0 where it is
/// clear of it or only touches): the deepest point of the surface inside the cutter, measured
/// to the cutter's own surface, which is the most the cutter shrunk all round (its tip as far
/// up the axis) still meets the surface. Found by halving, each step tested exactly against the
/// triangles, to a billionth of the radius. No point is deeper than the radius: a cutter that
/// deep in (buried in the part, plunged into a face) is reported as far as it would have to
/// back out along its axis to clear the surface, if that is farther.
pub fn depth_at(surface: &Surface, cutter: &Cutter, pose: &Pose) -> Result<f64> {
    cutter.validate()?;
    let meets = |d: f64| -> Result<bool> {
        // A tapered cutter keeps its angle, its side moved in (by at most half its radius).
        let (inner, lift) = match cutter.cone {
            None => (Cutter::bull(cutter.radius - d, (cutter.corner - d).max(0.0))?, d),
            Some(_) => cutter.shrunk(d)?,
        };
        Ok(drop_along_above(surface, &inner, pose.up(lift), pose.axis, 0.0)?.is_some())
    };
    let top = cutter.radius * (1.0 - 1e-9);
    if !meets(0.0)? {
        return Ok(0.0);
    }
    if meets(top)? {
        let out = drop_along(surface, cutter, pose.tip, pose.axis)?.unwrap_or(0.0);
        return Ok(cutter.radius.max(out));
    }
    let (mut lo, mut hi) = (0.0, top);
    for _ in 0..DEPTH_HALVINGS {
        let m = 0.5 * (lo + hi);
        if meets(m)? {
            lo = m;
        } else {
            hi = m;
        }
    }
    Ok(lo)
}

/// The nearest points of the segment `a–b` and the triangle `t`: the distance, the point on
/// the segment and the point on the triangle.
fn segment_triangle(a: Vec3, b: Vec3, t: &[Vec3; 3]) -> (f64, Vec3, Vec3) {
    if let Some(at) = ray_triangle(a, b - a, t)
        && at <= 1.0
    {
        let p = a.lerp(b, at);
        return (0.0, p, p);
    }
    let mut best = {
        let q = nearest_on_triangle(a, t);
        (q.dist(a), a, q)
    };
    let q = nearest_on_triangle(b, t);
    if q.dist(b) < best.0 {
        best = (q.dist(b), b, q);
    }
    for (p, q) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
        let (x, y) = nearest_segments(a, b, p, q);
        if x.dist(y) < best.0 {
            best = (x.dist(y), x, y);
        }
    }
    best
}

/// The distance between the triangles `t` and `q`, and the point of `q` nearest `t`: two
/// triangles apart are nearest along an edge of one of them, and two that meet have an edge of
/// one through the other.
fn triangle_triangle(t: &[Vec3; 3], q: &[Vec3; 3]) -> (f64, Vec3) {
    let mut best = (f64::INFINITY, q[0]);
    for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
        let (d, _, on_q) = segment_triangle(a, b, q);
        if d < best.0 {
            best = (d, on_q);
        }
    }
    for (a, b) in [(q[0], q[1]), (q[1], q[2]), (q[2], q[0])] {
        let (d, on_q, _) = segment_triangle(a, b, t);
        if d < best.0 {
            best = (d, on_q);
        }
    }
    best
}

/// For a ball moving straight from `from` to `to` with its axis held: the deepest the ball,
/// endless above its centre, is in the surface anywhere along the move, and the share of the
/// move where it is; `None` where it is clear all along. Exact: such a ball is everything within
/// its radius of the half-line up the axis from its centre, so along the move it sweeps
/// everything within the radius of the strip those half-lines sweep, and the deepest point is
/// the radius less the distance from the surface to the strip.
fn ball_move(surface: &Surface, r: f64, from: &Pose, to: &Pose) -> Option<(f64, f64)> {
    let a = from.axis;
    let (c0, c1) = (from.up(r), to.up(r));
    let bounds = surface.bounds();
    if bounds.is_empty() {
        return None;
    }
    // Up the axis to beyond the surface's highest point along it.
    let reach = [
        bounds.min,
        bounds.max,
        Vec3::new(bounds.max.x, bounds.min.y, bounds.min.z),
        Vec3::new(bounds.min.x, bounds.max.y, bounds.min.z),
        Vec3::new(bounds.min.x, bounds.min.y, bounds.max.z),
        Vec3::new(bounds.max.x, bounds.max.y, bounds.min.z),
        Vec3::new(bounds.max.x, bounds.min.y, bounds.max.z),
        Vec3::new(bounds.min.x, bounds.max.y, bounds.max.z),
    ]
    .iter()
    .map(|p| a.dot(*p))
    .fold(f64::NEG_INFINITY, f64::max)
        + r
        + 1.0;
    let (u0, u1) = ((reach - a.dot(c0)).max(0.0), (reach - a.dot(c1)).max(0.0));
    let along = c1 - c0;
    let across = along - a * a.dot(along);
    let strip: Vec<[Vec3; 3]> = if across.len() <= 1e-9 {
        // Along the axis: the strip is the half-line up from the lower centre.
        let (low, up) = if a.dot(c0) <= a.dot(c1) { (c0, u0) } else { (c1, u1) };
        vec![[low, low + a * up, low + a * up]]
    } else {
        let (q0, q1, q2, q3) = (c0, c1, c1 + a * u1, c0 + a * u0);
        vec![[q0, q1, q2], [q0, q2, q3]]
    };
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in strip.iter().flatten() {
        lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    let (lo, hi) = (lo - Vec3::new(r, r, r), hi + Vec3::new(r, r, r));
    let best: Cell<Option<(f64, Vec3)>> = Cell::new(None);
    surface.visit(
        |b| b.max.x >= lo.x && b.min.x <= hi.x && b.max.y >= lo.y && b.min.y <= hi.y && b.max.z >= lo.z && b.min.z <= hi.z,
        |_, t| {
            for q in &strip {
                let (d, at) = if across.len() <= 1e-9 {
                    let (d, on, _) = segment_triangle(q[0], q[1], t);
                    (d, on)
                } else {
                    triangle_triangle(t, q)
                };
                if d < r && best.get().is_none_or(|(b, _)| d < b) {
                    best.set(Some((d, at)));
                }
            }
        },
    );
    let (d, at) = best.get()?;
    let share = if across.len() <= 1e-9 { 0.0 } else { ((at - c0).dot(across) / across.len2()).clamp(0.0, 1.0) };
    Some((r - d, share))
}

/// The pose a share `t` of the way from `from` to `to`.
fn between(from: &Pose, to: &Pose, t: f64) -> Pose {
    Pose::new(from.tip.lerp(to.tip, t), slerp(from.axis, to.axis, t))
}

/// Finds gouges along moves, sampling where it cannot be exact.
struct Gouger<'a> {
    surface: &'a Surface,
    cutter: Cutter,
    test: GougeTest,
    tol: f64,
    step: f64,
}

impl Gouger<'_> {
    /// The deepest point of the move from `from` to `to` (with its share of the way) where the
    /// cutter is deeper than the tolerance, and how deep; the start is sampled only when
    /// `start` (it ends the move before otherwise).
    fn along(&self, from: &Pose, to: &Pose, start: bool) -> Result<Option<(f64, Pose, f64)>> {
        if self.cutter.is_ball() && from.axis_angle(to) <= 1e-12 {
            let Some((depth, t)) = ball_move(self.surface, self.cutter.radius, from, to).filter(|(depth, _)| *depth > self.tol) else {
                return Ok(None);
            };
            let pose = between(from, to, t);
            // As deep as the radius: the depth there says how far it is buried.
            let depth = if depth >= self.cutter.radius - 1e-12 { depth_at(self.surface, &self.cutter, &pose)? } else { depth };
            return Ok(Some((t, pose, depth)));
        }
        // Sampled at least every step, and between samples half way and near the thirds; the
        // samples the quick test finds in the surface are then measured, deepest by the quick
        // test first, and the deepest point near the deepest of them found.
        // Points of the end are up to its radius out and its rim height up: a tall cone's top
        // moves farther than its radius as the axis turns.
        let n = samples_along(from, to, self.cutter.radius.max(self.cutter.rim_height()), self.step)?;
        let mut found: Vec<(f64, f64)> = Vec::new();
        for k in 0..=n {
            let shares = std::iter::once(0.0).chain(if k < n { BETWEEN.to_vec() } else { Vec::new() });
            for s in shares {
                let t = (k as f64 + s) / n as f64;
                if t <= 0.0 && !start {
                    continue;
                }
                if let Some(d) = self.test.depth(self.surface, &between(from, to, t))? {
                    found.push((t, d));
                }
            }
        }
        found.sort_by(|a, b| b.1.total_cmp(&a.1));
        // The quick test is at least the depth where that is less than the radius (the cutter
        // shrunk by the depth is inside the one the quick test uses, moved up the axis by the
        // depth less the tolerance), so a sample it puts no deeper than the deepest measured is
        // no deeper; past the radius it says nothing, so a few more are measured.
        let mut worst: Option<(f64, f64)> = None;
        for (measured, (t, quick)) in found.into_iter().enumerate() {
            if let Some((_, w)) = worst
                && quick <= w
                && (w < self.cutter.radius * (1.0 - 1e-9) || measured >= MAX_BURIED_MEASURES)
            {
                break;
            }
            let depth = depth_at(self.surface, &self.cutter, &between(from, to, t))?;
            if worst.is_none_or(|(_, w)| depth > w) {
                worst = Some((t, depth));
            }
        }
        Ok(worst.filter(|(_, depth)| *depth > self.tol).map(|(t, depth)| (t, between(from, to, t), depth)))
    }

    /// The deepest point along the move near the share `t` (within a sample's spacing `w`
    /// either side), by golden-section search on the depth.
    fn deepest_near(&self, from: &Pose, to: &Pose, t: f64, w: f64, depth: f64) -> Result<(f64, Pose, f64)> {
        let g = 0.5 * (5f64.sqrt() - 1.0);
        let f = |s: f64| depth_at(self.surface, &self.cutter, &between(from, to, s));
        let (mut a, mut b) = ((t - w).max(0.0), (t + w).min(1.0));
        let (mut x1, mut x2) = (b - g * (b - a), a + g * (b - a));
        let (mut f1, mut f2) = (f(x1)?, f(x2)?);
        let mut best = (t, depth);
        for _ in 0..DEEPEST_STEPS {
            if f1 > best.1 {
                best = (x1, f1);
            }
            if f2 > best.1 {
                best = (x2, f2);
            }
            if f1 < f2 {
                a = x1;
                (x1, f1) = (x2, f2);
                x2 = a + g * (b - a);
                f2 = f(x2)?;
            } else {
                b = x2;
                (x2, f2) = (x1, f1);
                x1 = b - g * (b - a);
                f1 = f(x1)?;
            }
        }
        Ok((best.0, between(from, to, best.0), best.1))
    }
}

/// The poses along the cutting moves of `path` where `cutter` (as a cylinder of its radius,
/// endless above its end) cuts into the surface deeper than `tol`. One hit per move, at its
/// deepest point, its [`Hit::distance`] the depth there ([`depth_at`]): a cutter touching a
/// wall beside it, or within `tol` of it, does not gouge it. A ball whose axis holds along a
/// move is measured exactly along all of it; another cutter, or a turning axis, is tested at
/// samples at least every `sample_step` and between them (half way and near the thirds), the
/// deepest sample measured exactly, and the deepest hit of the path followed to the deepest
/// point near it.
pub fn gouges(surface: &Surface, cutter: &Cutter, path: &Toolpath5, tol: f64, sample_step: f64) -> Result<Vec<Hit>> {
    gouges_in(surface, cutter, path, tol, sample_step, Moves::Cutting)
}

/// Samples of a check below which one thread does it all: a thread costs more than that.
const SAMPLES_PER_THREAD: usize = 4096;

/// One move a check samples: its index, its start (`None`: the path's first pose, checked
/// where it stands), its end, whether its start is sampled (it ends an unchecked move) and how
/// many samples it takes.
#[derive(Clone, Copy)]
struct Leg {
    index: usize,
    from: Option<Pose>,
    to: Pose,
    start: bool,
    samples: usize,
}

/// The moves of `path` that `moves` selects, as [`Leg`]s, the samples of all of them counted
/// against [`MAX_SAMPLES`] (`per` samples a move of `n` steps takes).
fn legs(path: &Toolpath5, reach: f64, step: f64, moves: Moves, per: impl Fn(usize) -> usize) -> Result<(Vec<Leg>, usize)> {
    let mut out = Vec::new();
    let mut prev: Option<Pose> = None;
    let mut was_checked = false;
    let mut total = 0usize;
    for (i, m) in path.moves.iter().enumerate() {
        let (to, rapid) = match *m {
            Move5::Feed { to, .. } => (to, false),
            Move5::Rapid { to } => (to, true),
            Move5::Dwell { .. } => continue,
        };
        let checked = match moves {
            Moves::Cutting => !rapid,
            Moves::All => true,
        };
        if checked {
            let samples = match prev {
                Some(from) => {
                    let n = samples_along(&from, &to, reach, step)?;
                    total = total.saturating_add(per(n));
                    if total > MAX_SAMPLES {
                        return Err(CamError::TooLarge(format!("more than {MAX_SAMPLES} samples along the toolpath")));
                    }
                    n
                }
                None => 1,
            };
            out.push(Leg { index: i, from: prev, to, start: !was_checked, samples });
        }
        (was_checked, prev) = (checked, Some(to));
    }
    Ok((out, total))
}

/// What one move's gouge check found: where and how deep, and for a sampled move its ends,
/// share and sample spacing (to follow it to its deepest point).
type Found = Option<(Pose, f64, Option<(Pose, Pose, f64, f64)>)>;

/// [`gouges`] along the moves `moves` selects. The moves are checked on as many threads as the
/// work is worth ([`crate::par`]); what is found is the same as one after the other.
pub(crate) fn gouges_in(surface: &Surface, cutter: &Cutter, path: &Toolpath5, tol: f64, sample_step: f64, moves: Moves) -> Result<Vec<Hit>> {
    let test = GougeTest::new(cutter, tol)?;
    check_step(sample_step)?;
    check_path(path)?;
    let g = Gouger { surface, cutter: *cutter, test, tol, step: sample_step };
    let (legs, total) = legs(path, cutter.radius.max(cutter.rim_height()), sample_step, moves, |n| 4 * n + 1)?;
    let check = |leg: &Leg| -> Result<Found> {
        Ok(match leg.from {
            Some(from) => {
                let exact = cutter.is_ball() && from.axis_angle(&leg.to) <= 1e-12;
                g.along(&from, &leg.to, leg.start)?
                    .map(|(t, pose, depth)| (pose, depth, (!exact).then_some((from, leg.to, t, 1.0 / leg.samples as f64))))
            }
            None => {
                let depth = depth_at(surface, cutter, &leg.to)?;
                (depth > tol).then_some((leg.to, depth, None))
            }
        })
    };
    let weights: Vec<usize> = legs.iter().map(|l| l.samples + 1).collect();
    let ranges = crate::par::split(&weights, crate::par::threads_for(total, SAMPLES_PER_THREAD));
    let found: Vec<Vec<Found>> = crate::par::map(ranges, |r| legs.get(r).unwrap_or(&[]).iter().map(check).collect::<Result<Vec<_>>>())?;
    let mut hits: Vec<Hit> = Vec::new();
    // The deepest sampled hit: its index in `hits`, its move's ends, its share and spacing.
    let mut sampled: Option<(usize, Pose, Pose, f64, f64)> = None;
    for (leg, f) in legs.iter().zip(found.into_iter().flatten()) {
        let Some((pose, depth, at)) = f else { continue };
        if let Some((from, to, t, w)) = at
            && sampled.is_none_or(|(k, ..)| hits.get(k).is_none_or(|h| depth > h.distance))
        {
            sampled = Some((hits.len(), from, to, t, w));
        }
        hits.push(Hit { index: leg.index, pose, envelope: None, distance: depth });
    }
    // The deepest sampled hit, if it is the deepest of all, is followed to its deepest point.
    let deepest = hits.iter().map(|h| h.distance).fold(0.0, f64::max);
    if let Some((k, from, to, t, w)) = sampled
        && let Some(h) = hits.get_mut(k)
        && h.distance >= deepest
    {
        let (_, pose, depth) = g.deepest_near(&from, &to, t, w, h.distance)?;
        h.pose = pose;
        h.distance = depth;
    }
    Ok(hits)
}

/// A convex polygon of at most eight corners, without allocating.
struct Poly {
    p: [Vec3; 8],
    n: usize,
}

impl Poly {
    fn push(&mut self, v: Vec3) {
        if let Some(slot) = self.p.get_mut(self.n) {
            *slot = v;
            self.n += 1;
        }
    }

    fn points(&self) -> &[Vec3] {
        self.p.get(..self.n).unwrap_or(&[])
    }

    /// The part of the polygon where `h(p) ≥ 0`, `h` linear.
    fn clip(&self, h: impl Fn(Vec3) -> f64) -> Poly {
        let mut out = Poly { p: [Vec3::ZERO; 8], n: 0 };
        let pts = self.points();
        for (k, &a) in pts.iter().enumerate() {
            let b = pts.get(k + 1).or(pts.first()).copied().unwrap_or(a);
            let (ha, hb) = (h(a), h(b));
            if ha >= 0.0 {
                out.push(a);
            }
            if (ha >= 0.0) != (hb >= 0.0) {
                out.push(a + (b - a) * (ha / (ha - hb)));
            }
        }
        out
    }
}

/// The gap between the envelope's cylinder at `pose` and the triangle; `None` when no part of the
/// triangle is level with the cylinder (between its ends, along the axis).
fn cylinder_gap(pose: &Pose, e: &Envelope, t: &[Vec3; 3]) -> Option<f64> {
    let (a, o) = (pose.axis, pose.tip);
    let tri = Poly { p: [t[0], t[1], t[2], Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO], n: 3 };
    // Within the slab between the cylinder's ends, the distance to its axis segment is the
    // distance to its axis line, so the gap is exact.
    let slab = tri.clip(|p| a.dot(p - o) - e.bottom).clip(|p| e.top - a.dot(p - o));
    let (p0, p1) = (pose.up(e.bottom), pose.up(e.top));
    let d = match slab.points() {
        [] => return None,
        [q] => q.dist_to_segment(p0, p1),
        [q, r] => {
            let (x, y) = nearest_segments(*q, *r, p0, p1);
            x.dist(y)
        }
        [first, rest @ ..] => rest
            .windows(2)
            .map(|w| match w {
                [b, c] => segment_triangle_distance(p0, p1, &[*first, *b, *c]),
                _ => f64::INFINITY,
            })
            .fold(f64::INFINITY, f64::min),
    };
    Some(d - e.radius)
}

/// The smallest gap between the envelope at `pose` and the surface, when it is below `limit`.
fn envelope_gap(surface: &Surface, pose: &Pose, e: &Envelope, limit: f64) -> Option<f64> {
    let (p0, p1) = (pose.up(e.bottom), pose.up(e.top));
    let reach = e.radius + limit;
    let best: Cell<Option<f64>> = Cell::new(None);
    surface.visit(
        |b| capsule_meets_box(p0, p1, reach, b),
        |_, t| {
            if let Some(gap) = cylinder_gap(pose, e, t)
                && gap < limit
                && best.get().is_none_or(|b| gap < b)
            {
                best.set(Some(gap));
            }
        },
    );
    best.get()
}

/// The poses along the cutting moves of `path` where a part of `tool` above its flutes (each of
/// [`Tool::envelopes`]: the shank, then the holder's sections, cylinders about the axis) comes
/// closer to the surface than `clearance`, measured square to the axis. One hit per move and
/// envelope, at its closest sample.
pub fn holder_collisions(surface: &Surface, tool: &Tool, path: &Toolpath5, clearance: f64, sample_step: f64) -> Result<Vec<Hit>> {
    holder_collisions_in(surface, tool, path, clearance, 0.0, sample_step, Moves::Cutting)
}

/// [`holder_collisions`], allowing what the gouge check allows: an envelope no wider than the
/// cutter is inside the cutter's endless cylinder, so it reaches nothing the cutter does not
/// reach first, and a wall the cutter runs along within `tolerance` (one it has just cut) is no
/// collision for it either. Such an envelope counts only where it comes closer than `clearance`
/// less `tolerance`; a wider one as in [`holder_collisions`].
pub fn holder_collisions_within(
    surface: &Surface,
    tool: &Tool,
    path: &Toolpath5,
    clearance: f64,
    tolerance: f64,
    sample_step: f64,
) -> Result<Vec<Hit>> {
    if !(tolerance >= 0.0) || !(tolerance <= 1.0e3) {
        return invalid("the gouge tolerance must be from 0 to 1000 mm");
    }
    holder_collisions_in(surface, tool, path, clearance, tolerance, sample_step, Moves::Cutting)
}

/// [`holder_collisions_within`] along the moves `moves` selects.
pub(crate) fn holder_collisions_in(
    surface: &Surface,
    tool: &Tool,
    path: &Toolpath5,
    clearance: f64,
    tolerance: f64,
    sample_step: f64,
    moves: Moves,
) -> Result<Vec<Hit>> {
    tool.validate()?;
    if !(clearance >= 0.0) || !(clearance <= 1.0e4) {
        return invalid("the holder clearance must be from 0 to 10 m");
    }
    check_step(sample_step)?;
    check_path(path)?;
    let envelopes = tool.envelopes();
    let reach = envelopes.iter().map(|e| e.top).fold(0.0, f64::max);
    let mut hits: Vec<Hit> = Vec::new();
    if envelopes.is_empty() {
        return Ok(hits);
    }
    // What each envelope must keep from the surface.
    let limits: Vec<f64> = envelopes.iter().map(|e| if e.radius <= tool.radius() + 1e-9 { clearance - tolerance } else { clearance }).collect();
    // The closest sample of each envelope on each move, the moves on as many threads as the
    // work is worth: the same hits as one sample after the other.
    let (legs, total) = legs(path, reach, sample_step, moves, |n| n + 1)?;
    let check = |leg: &Leg| -> Vec<Hit> {
        let mut out: Vec<Hit> = Vec::new();
        let mut at = |pose: Pose| {
            for (k, (e, limit)) in envelopes.iter().zip(&limits).enumerate() {
                let Some(gap) = envelope_gap(surface, &pose, e, *limit) else { continue };
                let hit = Hit { index: leg.index, pose, envelope: Some(k), distance: gap };
                match out.iter_mut().find(|h| h.envelope == Some(k)) {
                    Some(h) => {
                        if gap < h.distance {
                            *h = hit;
                        }
                    }
                    None => out.push(hit),
                }
            }
        };
        match leg.from {
            Some(from) => {
                let n = leg.samples;
                for k in usize::from(!leg.start)..=n {
                    let t = k as f64 / n as f64;
                    at(Pose::new(from.tip.lerp(leg.to.tip, t), slerp(from.axis, leg.to.axis, t)));
                }
            }
            None => at(leg.to),
        }
        out
    };
    let weights: Vec<usize> = legs.iter().map(|l| l.samples + 1).collect();
    let ranges = crate::par::split(&weights, crate::par::threads_for(total, SAMPLES_PER_THREAD));
    let found: Vec<Vec<Hit>> = crate::par::map(ranges, |r| Ok(legs.get(r).unwrap_or(&[]).iter().flat_map(check).collect()))?;
    hits.extend(found.into_iter().flatten());
    Ok(hits)
}

/// Which way [`tilt_away`] leans the tool.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TiltAbout {
    /// Sideways, about the feed direction, to whichever side (the same along each run of cutting
    /// moves) clears with the smaller tilt.
    FeedSide,
    /// The spindle towards `point`.
    TowardPoint { point: Vec3 },
    /// The spindle away from `point`.
    AwayFromPoint { point: Vec3 },
}

/// How [`tilt_away`] tilts a toolpath clear.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TiltOptions {
    /// Most the tool may be tilted from the path's own axis (radians, at most a right angle).
    pub max_tilt: f64,
    /// The tilt is tried in increments of this (radians).
    pub step: f64,
    pub about: TiltAbout,
    /// Least gap between the shank or holder and the surface (mm).
    pub clearance: f64,
    /// Most the tool axis may turn in one cutting move (radians).
    pub max_axis_step: f64,
    /// The path is divided into moves of about this length and checked this finely (mm).
    pub sample_step: f64,
    /// How far the cutter may cut into the surface before the result is refused (mm).
    pub tolerance: f64,
}

impl TiltOptions {
    pub fn validate(&self) -> Result<()> {
        if !(self.max_tilt > 0.0) || !(self.max_tilt <= std::f64::consts::FRAC_PI_2) {
            return invalid("the largest tilt must be above 0 and at most a right angle");
        }
        if !(self.step >= MIN_TILT_STEP) || !(self.step <= self.max_tilt) {
            return invalid("the tilt increment must be from 0.0001 rad to the largest tilt");
        }
        if !(self.max_axis_step >= MIN_TILT_STEP) || !(self.max_axis_step <= std::f64::consts::PI) {
            return invalid("the largest axis step must be from 0.0001 rad to π");
        }
        if !(self.clearance >= 0.0) || !(self.clearance <= 1.0e4) {
            return invalid("the holder clearance must be from 0 to 10 m");
        }
        if !(self.tolerance >= 0.0) || !(self.tolerance <= 1.0e3) {
            return invalid("the gouge tolerance must be from 0 to 1000 mm");
        }
        if let TiltAbout::TowardPoint { point } | TiltAbout::AwayFromPoint { point } = self.about
            && !point.is_finite()
        {
            return invalid("the point the tool is tilted by must be finite");
        }
        check_step(self.sample_step)
    }
}

/// One move of the path being tilted.
#[derive(Clone, Copy, Debug)]
enum Node {
    Rapid { pose: Pose, from: usize },
    Feed { pose: Pose, feed: f64, from: usize },
    Dwell { seconds: f64, from: usize },
}

impl Node {
    fn pose(&self) -> Option<Pose> {
        match *self {
            Node::Rapid { pose, .. } | Node::Feed { pose, .. } => Some(pose),
            Node::Dwell { .. } => None,
        }
    }

    fn from(&self) -> usize {
        match *self {
            Node::Rapid { from, .. } | Node::Feed { from, .. } | Node::Dwell { from, .. } => from,
        }
    }
}

/// The path with every cutting move divided into pieces of at most `step` (tip and the top of
/// the tool).
fn divide(path: &Toolpath5, reach: f64, step: f64) -> Result<Vec<Node>> {
    let mut out = Vec::with_capacity(path.moves.len());
    let mut prev: Option<Pose> = None;
    for (i, m) in path.moves.iter().enumerate() {
        match *m {
            Move5::Rapid { to } => out.push(Node::Rapid { pose: to, from: i }),
            Move5::Feed { to, feed } => {
                let n = match prev {
                    Some(from) => samples_along(&from, &to, reach, step)?,
                    None => 1,
                };
                if out.len().saturating_add(n) > MAX_MOVES {
                    return Err(CamError::TooLarge(format!("the divided toolpath has more than {MAX_MOVES} moves")));
                }
                for k in 1..=n {
                    let t = k as f64 / n as f64;
                    let pose = match prev {
                        Some(from) if k < n => Pose::new(from.tip.lerp(to.tip, t), slerp(from.axis, to.axis, t)),
                        _ => to,
                    };
                    out.push(Node::Feed { pose, feed, from: i });
                }
            }
            Move5::Dwell { seconds } => out.push(Node::Dwell { seconds, from: i }),
        }
        if let Some(to) = m.target() {
            prev = Some(to);
        }
    }
    Ok(out)
}

struct Tilter<'a> {
    surface: &'a Surface,
    cutter: Cutter,
    envelopes: Vec<Envelope>,
    clearance: f64,
    /// How far the cutter may cut into the surface (mm).
    tol: f64,
}

impl Tilter<'_> {
    fn collides(&self, pose: &Pose) -> bool {
        self.envelopes.iter().any(|e| envelope_gap(self.surface, pose, e, self.clearance).is_some())
    }

    /// `pose` with its axis leant by `angle` towards the unit `d` (square to the axis), keeping
    /// the cutter where it was: a ball turns about its centre, which keeps it touching the same
    /// points; another end turns about its tip and is pushed out along the new axis as far as it
    /// would cut in (never pulled in, so a pose in the air stays in the air). `None` where that
    /// would lift it off what it cuts ([`Tilter::turned`]).
    fn lean(&self, pose: &Pose, d: Vec3, angle: f64) -> Result<Option<Pose>> {
        if angle <= 0.0 {
            return Ok(Some(*pose));
        }
        let a = (pose.axis * angle.cos() + d * angle.sin()).normalized().unwrap_or(pose.axis);
        self.turned(pose, a)
    }

    /// `pose` with its axis `a`, the cutter kept as [`Tilter::lean`] does. `None` where an end
    /// other than a ball would have to be pushed out farther than turning it can ask of the
    /// surface it touches: its endless side has then met something beside it (a wall it runs
    /// along), and pushed out of that, it would no longer cut where it should, but somewhere up
    /// the wall or in the air.
    fn turned(&self, pose: &Pose, a: Vec3) -> Result<Option<Pose>> {
        if self.cutter.is_ball() {
            let r = self.cutter.radius;
            return Ok(Some(Pose::new(pose.up(r) - a * r, a)));
        }
        let out = drop_along(self.surface, &self.cutter, pose.tip, a)?.filter(|t| *t > 0.0).unwrap_or(0.0);
        // Turned by θ about the tip, no point of the end (none farther from the tip than
        // hypot(radius, rim height)) moves by more than 2·hypot·sin(θ/2); pushing it out along the
        // new axis clears that from a surface up to 60° from the axis within twice as far.
        let theta = a.cross(pose.axis).len().atan2(a.dot(pose.axis));
        let limit = 4.0 * self.cutter.radius.hypot(self.cutter.rim_height()) * (theta * 0.5).sin() + self.tol + 1e-9;
        Ok((out <= limit).then(|| Pose::new(pose.tip + a * out, a)))
    }

    /// The least tilt towards `d` in whole increments (the last one the largest tilt) that clears
    /// `pose`, and one increment more when the largest tilt allows it: the moves between poses,
    /// where the direction leant towards changes, are then clear too. `None` when none clears.
    fn needed(&self, pose: &Pose, d: Vec3, opts: &TiltOptions) -> Result<Option<f64>> {
        let n = (opts.max_tilt / opts.step - 1e-9).ceil().max(1.0) as usize;
        for k in 1..=n {
            let angle = (k as f64 * opts.step).min(opts.max_tilt);
            if self.lean(pose, d, angle)?.is_some_and(|p| !self.collides(&p)) {
                return Ok(Some((angle + opts.step).min(opts.max_tilt)));
            }
        }
        Ok(None)
    }
}

/// The unit direction of `v` square to the unit axis `a`, if it has one.
fn square_to(v: Vec3, a: Vec3) -> Option<Vec3> {
    let w = v - a * a.dot(v);
    (w.len() > 1e-9).then(|| w.normalized()).flatten()
}

/// The toolpath `path` (for example a 3-axis one through [`Toolpath5::from_3axis`]) made clear
/// of the tool's shank and holder by tilting it: at every pose where they come closer to the
/// surface than the clearance, the axis is leant ([`TiltAbout`]) in increments until they are
/// clear, keeping the cutter on the same points (a ball turns about its centre; other ends turn
/// about the tip, pushed out as far as they would cut in, and a lean that would push them out
/// farther, off what they cut, does not count as clear). The tilt is then spread along the path
/// so that it grows before a collision and dies away after it, the axis never turning more than
/// the step allowed in one move. A rapid move does not turn the axis on its way: the axis turns
/// where the tool stands, at the end of a rapid away from the part or at the start of one
/// towards it. The result is checked along every move, the rapids too: no gouge beyond the
/// tolerance, no holder collision, and [`verify5`]. Refused with [`CamError::Unsafe`], naming
/// the move of `path`, where even the largest tilt does not clear or the result fails a check.
pub fn tilt_away(surface: &Surface, tool: &Tool, path: &Toolpath5, opts: &TiltOptions) -> Result<Toolpath5> {
    tool.validate()?;
    opts.validate()?;
    check_path(path)?;
    let t = Tilter { surface, cutter: tool.cutter()?, envelopes: tool.envelopes(), clearance: opts.clearance, tol: opts.tolerance };
    if t.envelopes.is_empty() {
        return Ok(path.clone());
    }
    let reach = t.envelopes.iter().map(|e| e.top).fold(t.cutter.radius, f64::max);
    let nodes = divide(path, reach, opts.sample_step)?;
    // Runs of moves the tilt is spread along: each starts at a rapid and holds its cutting moves.
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for (k, node) in nodes.iter().enumerate() {
        match node {
            Node::Rapid { .. } => runs.push(vec![k]),
            Node::Feed { .. } => match runs.last_mut() {
                Some(run) => run.push(k),
                None => runs.push(vec![k]),
            },
            Node::Dwell { .. } => {}
        }
    }
    let mut leant: Vec<Option<Pose>> = nodes.iter().map(Node::pose).collect();
    let spread = opts.max_axis_step * 0.5;
    for run in &runs {
        let poses: Vec<(usize, Pose)> = run.iter().filter_map(|&k| nodes.get(k).and_then(|n| n.pose().map(|p| (k, p)))).collect();
        let hit: Vec<bool> = poses.iter().map(|(_, p)| t.collides(p)).collect();
        if !hit.contains(&true) {
            continue;
        }
        // The direction to lean each pose towards.
        let feed_dir = |j: usize| -> Vec3 {
            let at = |i: usize| poses.get(i).map(|(_, p)| *p);
            let Some(p) = at(j) else { return Vec3::new(1.0, 0.0, 0.0) };
            let way = |i: usize, k: usize| match (at(i), at(k)) {
                (Some(a), Some(b)) => square_to(b.tip - a.tip, p.axis),
                _ => None,
            };
            let near = j.checked_sub(1).and_then(|i| way(i, j)).or_else(|| way(j, j + 1));
            let later = || (j + 1..poses.len()).find_map(|i| way(i - 1, i));
            let earlier = || (1..j).rev().find_map(|i| way(i - 1, i));
            near.or_else(later).or_else(earlier).unwrap_or_else(|| p.axis.any_perp())
        };
        let dirs = |sign: f64| -> Vec<Vec3> {
            poses
                .iter()
                .enumerate()
                .map(|(j, (_, p))| {
                    let d = match opts.about {
                        TiltAbout::FeedSide => square_to(p.axis.cross(feed_dir(j)) * sign, p.axis),
                        TiltAbout::TowardPoint { point } => square_to(point - p.tip, p.axis),
                        TiltAbout::AwayFromPoint { point } => square_to(p.tip - point, p.axis),
                    };
                    d.unwrap_or_else(|| p.axis.any_perp())
                })
                .collect()
        };
        let needs = |d: &[Vec3]| -> Result<std::result::Result<Vec<f64>, usize>> {
            let mut out = Vec::with_capacity(poses.len());
            for (j, ((k, p), dir)) in poses.iter().zip(d).enumerate() {
                if !hit.get(j).copied().unwrap_or(false) {
                    out.push(0.0);
                    continue;
                }
                match t.needed(p, *dir, opts)? {
                    Some(a) => out.push(a),
                    None => return Ok(Err(*k)),
                }
            }
            Ok(Ok(out))
        };
        let signs: &[f64] = if opts.about == TiltAbout::FeedSide { &[1.0, -1.0] } else { &[1.0] };
        let mut best: Option<(f64, Vec<Pose>)> = None;
        let mut stuck = 0;
        'signs: for &sign in signs {
            let d = dirs(sign);
            let mut angles = match needs(&d)? {
                Ok(angles) => angles,
                Err(k) => {
                    stuck = k;
                    continue;
                }
            };
            // Spread the tilt: lean neighbours early and ease off after.
            for j in 1..angles.len() {
                let before = angles.get(j - 1).copied().unwrap_or(0.0);
                if let Some(a) = angles.get_mut(j) {
                    *a = a.max(before - spread);
                }
            }
            for j in (0..angles.len().saturating_sub(1)).rev() {
                let after = angles.get(j + 1).copied().unwrap_or(0.0);
                if let Some(a) = angles.get_mut(j) {
                    *a = a.max(after - spread);
                }
            }
            let worst = angles.iter().copied().fold(0.0, f64::max);
            if best.as_ref().is_some_and(|(w, _)| *w <= worst) {
                continue;
            }
            // Every pose of the run, the spread ones too, must take its lean.
            let mut out = Vec::with_capacity(poses.len());
            for (((k, p), dir), angle) in poses.iter().zip(&d).zip(&angles) {
                match t.lean(p, *dir, *angle)? {
                    Some(q) => out.push(q),
                    None => {
                        stuck = *k;
                        continue 'signs;
                    }
                }
            }
            best = Some((worst, out));
        }
        let Some((_, out)) = best else {
            let from = nodes.get(stuck).map_or(0, Node::from);
            return Err(CamError::Unsafe(format!("move {from}: the holder cannot be tilted clear of the surface within the largest tilt")));
        };
        for ((k, _), q) in poses.iter().zip(out) {
            if let Some(slot) = leant.get_mut(*k) {
                *slot = Some(q);
            }
        }
    }
    // The moves, with poses turned evenly in between where the axis still turns too far.
    let mut moves: Vec<Move5> = Vec::with_capacity(nodes.len());
    let mut from: Vec<usize> = Vec::with_capacity(nodes.len());
    let mut prev: Option<(Pose, Pose)> = None;
    for (node, new) in nodes.iter().zip(&leant) {
        let (Some(orig), Some(new)) = (node.pose(), *new) else {
            if let Node::Dwell { seconds, from: f } = *node {
                moves.push(Move5::Dwell { seconds });
                from.push(f);
            }
            continue;
        };
        if let (Node::Feed { feed, from: f, .. }, Some((po, pn))) = (*node, prev) {
            let angle = pn.axis_angle(&new);
            if angle > opts.max_axis_step {
                let n = (angle / opts.max_axis_step).ceil() as usize;
                if moves.len().saturating_add(n) > MAX_MOVES {
                    return Err(CamError::TooLarge(format!("the tilted toolpath has more than {MAX_MOVES} moves")));
                }
                for i in 1..n {
                    let s = i as f64 / n as f64;
                    let base = Pose::new(po.tip.lerp(orig.tip, s), slerp(po.axis, orig.axis, s));
                    let Some(to) = t.turned(&base, slerp(pn.axis, new.axis, s))? else {
                        return Err(CamError::Unsafe(format!("move {f}: turning the tool there would lift the cutter off what it cuts")));
                    };
                    moves.push(Move5::Feed { to, feed });
                    from.push(f);
                }
            }
        }
        // A rapid turns the axis where the tool stands, never on its way: at its end when it
        // leaves the part (along the axes), at its start when it comes towards it.
        if let (Node::Rapid { .. }, Some((_, pn))) = (*node, prev)
            && pn.axis_angle(&new) > 1e-12
            && pn.tip.dist(new.tip) > 1e-12
        {
            if moves.len() >= MAX_MOVES {
                return Err(CamError::TooLarge(format!("the tilted toolpath has more than {MAX_MOVES} moves")));
            }
            let away = (new.tip - pn.tip).dot(pn.axis + new.axis) >= 0.0;
            moves.push(Move5::Rapid { to: if away { Pose::new(new.tip, pn.axis) } else { Pose::new(pn.tip, new.axis) } });
            from.push(node.from());
        }
        if moves.len() >= MAX_MOVES {
            return Err(CamError::TooLarge(format!("the tilted toolpath has more than {MAX_MOVES} moves")));
        }
        moves.push(match *node {
            Node::Feed { feed, .. } => Move5::Feed { to: new, feed },
            _ => Move5::Rapid { to: new },
        });
        from.push(node.from());
        prev = Some((orig, new));
    }
    let out = Toolpath5 { moves };
    let named = |i: usize| from.get(i).copied().unwrap_or(0);
    verify5(&out, opts.max_axis_step).map_err(|e| CamError::Unsafe(format!("the tilted toolpath fails its check: {e}")))?;
    if let Some(h) = gouges_in(surface, &t.cutter, &out, opts.tolerance, opts.sample_step, Moves::All)?.first() {
        return Err(CamError::Unsafe(format!("move {}: the tilted tool cuts {:.4} mm into the surface", named(h.index), h.distance)));
    }
    if let Some(h) = holder_collisions_in(surface, tool, &out, opts.clearance, 0.0, opts.sample_step, Moves::All)?.first() {
        return Err(CamError::Unsafe(format!("move {}: the tilted holder still comes within {:.4} mm of the surface", named(h.index), h.distance)));
    }
    Ok(out)
}
