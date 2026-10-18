//! The program as written, read back and held against the toolpaths it was written from, so
//! what leaves for the machine is checked rather than what was planned. Rounding to the post's
//! decimals, arcs written as chords, and anything lost between the two show up as distance
//! between the cutting moves of the two.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

use crate::gcode::{Program, parse};
use crate::path::Toolpath;
use crate::post::Post;
use crate::{CamError, Result};

/// Most grid entries the comparison builds.
const MAX_ENTRIES: usize = 20_000_000;

/// The written program against its toolpaths.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Readback {
    /// Largest distance between a cutting move as written and as planned, either way round
    /// (mm). Distances beyond [`Readback::reach`] are reported as `reach`.
    pub max_deviation: f64,
    /// How far the post's own precision lets the two differ (mm): see [`allowed_deviation`].
    pub allowed: f64,
    /// Farthest the comparison looks (mm).
    pub reach: f64,
    /// Distance at feed as written and as planned (mm).
    pub cut_length: f64,
    pub planned_cut_length: f64,
    /// Time at feed as written and as planned, dwells included (minutes).
    pub cut_minutes: f64,
    pub planned_cut_minutes: f64,
}

impl Readback {
    /// Whether the written program stays within the post's precision of its toolpaths.
    pub fn within(&self) -> bool {
        self.max_deviation <= self.allowed
    }
}

/// The largest distance between a written and a planned cutting move that `post`'s precision
/// accounts for (mm): half the last written digit on each of the three axes, for an arc's end
/// and its centre (whose rounding can change the radius by both); the chord error of arcs
/// written as lines; and the same chord error again where the comparison follows arcs.
pub fn allowed_deviation(post: &Post) -> f64 {
    let digit = 0.5 * 10f64.powi(-i32::from(post.decimals)) * post.units.mm();
    3.0 * digit * 3f64.sqrt() + 2.0 * post.arc_tolerance + 1e-6
}

/// Reads `text` (written by [`crate::emit`] from `program` through `post`) back and measures how
/// far its cutting moves are from the program's toolpaths.
pub fn readback(text: &str, program: &Program, post: &Post) -> Result<Readback> {
    let written = Toolpath { moves: parse(text)? };
    let planned = Toolpath { moves: program.operations.iter().flat_map(|op| op.path.moves.iter().copied()).collect() };
    let allowed = allowed_deviation(post);
    let reach = (allowed * 10.0).max(1.0);
    let tol = post.arc_tolerance;
    let (a, b) = (cutting_segments(&written, tol), cutting_segments(&planned, tol));
    let max_deviation = directed(&a, &b, reach)?.max(directed(&b, &a, reach)?);
    let (ws, ps) = (written.stats(), planned.stats());
    Ok(Readback {
        max_deviation,
        allowed,
        reach,
        cut_length: ws.cut_length,
        planned_cut_length: ps.cut_length,
        cut_minutes: ws.cut_minutes,
        planned_cut_minutes: ps.cut_minutes,
    })
}

/// The pieces the tool travels at feed, arcs followed within `tol`.
fn cutting_segments(path: &Toolpath, tol: f64) -> Vec<(Vec3, Vec3)> {
    let mut out = Vec::new();
    let mut prev: Option<Vec3> = None;
    for (p, cutting) in path.points(tol) {
        if let (Some(a), true) = (prev, cutting) {
            out.push((a, p));
        }
        prev = Some(p);
    }
    out
}

/// The largest distance from a point of `from` (both ends of each piece, and its middle) to the
/// nearest piece of `to`, capped at `reach`.
fn directed(from: &[(Vec3, Vec3)], to: &[(Vec3, Vec3)], reach: f64) -> Result<f64> {
    if from.is_empty() {
        return Ok(0.0);
    }
    if to.is_empty() {
        return Ok(reach);
    }
    // Pieces of `to` filed by the XY cells their box covers; a point looks in its cell and the
    // eight around it, so any piece within `cell` of it in XY is seen.
    let (mut lo, mut hi) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    for &(a, b) in to.iter().chain(from) {
        lo = lo.min(a).min(b);
        hi = hi.max(a).max(b);
    }
    if !(lo.is_finite() && hi.is_finite()) {
        return Err(CamError::Unsafe("a toolpath has a coordinate that is not a finite number".into()));
    }
    let cell = reach.max((hi.x - lo.x).max(hi.y - lo.y) / 4096.0);
    let index = |v: f64, o: f64| ((v - o) / cell).floor() as i64;
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut entries = 0usize;
    for (k, &(a, b)) in to.iter().enumerate() {
        let (i0, i1) = (index(a.x.min(b.x), lo.x), index(a.x.max(b.x), lo.x));
        let (j0, j1) = (index(a.y.min(b.y), lo.y), index(a.y.max(b.y), lo.y));
        entries = entries.saturating_add(usize::try_from((i1 - i0 + 1) * (j1 - j0 + 1)).unwrap_or(usize::MAX));
        if entries > MAX_ENTRIES {
            return Err(CamError::TooLarge("the program is too large to read back and check".into()));
        }
        for i in i0..=i1 {
            for j in j0..=j1 {
                grid.entry((i, j)).or_default().push(k);
            }
        }
    }
    let nearest = |p: Vec3| -> f64 {
        let (pi, pj) = (index(p.x, lo.x), index(p.y, lo.y));
        let mut best = reach;
        for i in pi - 1..=pi + 1 {
            for j in pj - 1..=pj + 1 {
                for &k in grid.get(&(i, j)).map(Vec::as_slice).unwrap_or_default() {
                    if let Some(&seg) = to.get(k) {
                        best = best.min(p.dist_to_segment(seg.0, seg.1));
                    }
                }
            }
        }
        best
    };
    let mut worst: f64 = 0.0;
    for &(a, b) in from {
        for p in [a, (a + b) * 0.5, b] {
            worst = worst.max(nearest(p));
            if worst >= reach {
                return Ok(reach);
            }
        }
    }
    Ok(worst)
}
