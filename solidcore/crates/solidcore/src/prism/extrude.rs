//! A planar region extruded along its frame's normal between two levels: the prism the planar
//! extrusion of `solidcore-ops` joins its pieces from.
//!
//! Not upstream: the placement SolidCraft's extrusion of planar regions made (`crate::extrude`
//! for one region), moved into the kernel (docs/layers.md §1). The solid is the kernel's linear
//! sweep (`SmApiCreateLinearSweep`) of the region's curves.
//!
//! The levels of an extrude's ends on planes at a slant to it ([`level_on_plane`],
//! [`plane_rise`], [`planes_cross_over`]) were SolidCraft's extrude From/To code, moved into the
//! kernel the same way (#21934).
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use super::profile::{axes, curve_on_frame, halves, point_of, to_world, Region2, Seg2, H2};
use crate::error::{Result, SolidError};
use crate::eval::Frame;
use crate::geom::vector3d::Point3;
use crate::kernel::Kernel;
use crate::math::{Point2, Vec2};
use crate::sweep::api_brep::create_linear_sweep;
use crate::topology::Brep;

const OP: &str = "prism::extrude::extrude_region";

/// The solid of `region` (in `frame`'s coordinates) from level `lo` to level `hi` above the
/// frame: its loops' curves placed on the frame at `lo` ([`curve_on_frame`]) and swept along the
/// frame's normal by `hi - lo`, capped at both ends.
///
/// # Errors
///
/// `InvalidInput` for levels not finite or not increasing; those of the curves and the sweep.
pub fn extrude_region(
    k: &mut Kernel,
    frame: &Frame,
    region: &Region2,
    lo: f64,
    hi: f64,
) -> Result<Brep> {
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Err(SolidError::invalid_input(
            OP,
            "levels not finite and increasing",
        ));
    }
    let curves = region
        .loops()
        .flat_map(|lp| &lp.segs)
        .map(|s| curve_on_frame(k, frame, lo, s))
        .collect::<Result<Vec<_>>>()?;
    let refs: Vec<_> = curves.iter().map(Some).collect();
    let (_, _, _, n) = axes(frame);
    create_linear_sweep(k, &refs, n, hi - lo, true)
}

/// Positive and negative wing `(start, end)` intervals for a symmetric-offset extrude with
/// clearance `gap` on each side of the symmetry plane and `half` distance per wing.
#[must_use]
pub fn symmetric_offset_intervals(gap: f64, half: f64) -> [(f64, f64); 2] {
    let gap = gap.abs();
    [(gap, gap + half), (-gap - half, -gap)]
}

/// Probe points along both wings of a symmetric-offset extrude for auto-operation detection
/// (cut vs join), given the sketch-plane offset `start`, total extent `d`, and `near_limit`.
#[must_use]
pub fn symmetric_offset_probe_points(
    base: [f64; 3],
    n: [f64; 3],
    start: f64,
    d: f64,
    near_limit: f64,
) -> [[f64; 3]; 4] {
    let gap = start.abs();
    let half = d.abs() * 0.5;
    let near = (half * 0.5).min(near_limit);
    let mid = half * 0.5;
    let at = |s: f64| [base[0] + n[0] * s, base[1] + n[1] * s, base[2] + n[2] * s];
    [
        at(-start + gap + near),
        at(-start + gap + mid),
        at(-start - gap - near),
        at(-start - gap - mid),
    ]
}

/// Where the line along `frame`'s normal through `point` meets `plane` (the plane through
/// `plane`'s origin square to its normal `x × y`), as a level along that normal measured from
/// `frame`'s origin: the meet's [`super::profile::height`]. `None` when the plane runs along the
/// normal (their dot product under 1e-9), which the line never meets, or for a level that is
/// not finite.
#[must_use]
pub fn level_on_plane(frame: &Frame, point: Point3, plane: &Frame) -> Option<f64> {
    let (o, _, _, n) = axes(frame);
    let (plane_origin, _, _, m) = axes(plane);
    let across = n.dot(m);
    if !(across.abs() >= 1e-9) {
        return None;
    }
    let level = (point + n * ((plane_origin - point).dot(m) / across) - o).dot(n);
    level.is_finite().then_some(level)
}

/// The least and the most level of `plane` above `frame` ([`level_on_plane`]) over the outer
/// loops of `regions` (in `frame`'s coordinates): the rise of a plane at a slant to the
/// extrusion across the profiles. Exact for lines and arcs; a free-form piece is searched to a
/// hair ([`affine_extremes`]). `None` with no outer segment, for a plane along the normal, or for
/// a malformed segment.
#[must_use]
pub fn plane_rise(frame: &Frame, plane: &Frame, regions: &[Region2]) -> Option<(f64, f64)> {
    affine_extremes(regions, |q| {
        level_on_plane(frame, to_world(frame, q, 0.0), plane)
    })
}

/// Whether `to` lies before `from` along the extrusion (`frame`'s normal times the sign of
/// `way`) somewhere over the outer loops of `regions`: the two planes cross over the profiles,
/// so part of them would have no extent. The gap between the planes changes linearly across
/// the sketch, so its least value lies at a line's end or at an arc's end or its point
/// farthest the gap's way down; a free-form piece is searched to a hair ([`affine_extremes`]).
/// A crossing within `1e-9 (1 + reach)` of a loop, `reach` the gap's largest size over the
/// loops, does not count. `None` as [`plane_rise`].
#[must_use]
pub fn planes_cross_over(
    frame: &Frame,
    from: &Frame,
    to: &Frame,
    regions: &[Region2],
    way: f64,
) -> Option<bool> {
    if !way.is_finite() {
        return None;
    }
    let (least, most) = affine_extremes(regions, |q| {
        let q = to_world(frame, q, 0.0);
        Some((level_on_plane(frame, q, to)? - level_on_plane(frame, q, from)?) * way.signum())
    })?;
    let reach = least.abs().max(most.abs());
    Some(least < -1e-9 * (1.0 + reach))
}

/// Halvings of a free-form span past which [`affine_extremes`] stops refining it: a piece
/// 2^-48 of its span's parameter, far below what the hair below can tell apart.
const MAX_HALVINGS: u32 = 48;

/// How close, relative to the values' size, a free-form piece's enclosure must come to the
/// extremes found on it before [`affine_extremes`] stops halving it. It is a stop rule, not a
/// bound: what is returned is always a value the curve takes, so the hair only decides how close
/// to the true extreme the search goes, a thousand times under the `1e-9` a crossing in
/// [`planes_cross_over`] must clear.
const HAIR: f64 = 1e-12;

/// The least and the most of `g` over the outer loops of `regions`, `g` affine over the sketch
/// (a plane's level, or the gap between two). Its extremes over a line lie at the line's ends,
/// over an arc at its ends or at its points farthest along and against `g`'s gradient where the
/// arc passes them, `g` evaluated at exactly those points. Over a free-form piece (cubic, conic,
/// Bézier) `g`'s values at its homogeneous control points enclose it (the convex hull property,
/// the weights positive), so each span is halved until its enclosure lies within [`HAIR`] of
/// the extremes found at the ends of its pieces, which lie on the curve: the extremes returned
/// are values `g` takes. `None` with no outer segment, where `g` is `None` or
/// not finite, or for a malformed segment.
fn affine_extremes(regions: &[Region2], g: impl Fn(Point2) -> Option<f64>) -> Option<(f64, f64)> {
    let g = |q: Point2| g(q).filter(|v| v.is_finite());
    let at0 = g(Vec2::new(0.0, 0.0))?;
    let slope = Vec2::new(g(Vec2::new(1.0, 0.0))? - at0, g(Vec2::new(0.0, 1.0))? - at0);
    // the angles of the sketch directions along which `g` falls and rises fastest
    let fall = (slope.x != 0.0 || slope.y != 0.0).then(|| (-slope.y).atan2(-slope.x));
    let mut range: Option<(f64, f64)> = None;
    let mut take = |v: f64| {
        let (lo, hi) = range.unwrap_or((v, v));
        range = Some((lo.min(v), hi.max(v)));
    };
    for seg in regions.iter().flat_map(|r| &r.outer.segs) {
        match *seg {
            Seg2::Line { a, b } => {
                take(g(a)?);
                take(g(b)?);
            }
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                take(g(seg.start())?);
                take(g(seg.end())?);
                for angle in fall.into_iter().flat_map(|f| [f, f + std::f64::consts::PI]) {
                    // The arc's point at angle `t` is `center + (cos t, sin t) radius`, so with a
                    // negative radius the point farthest along a direction lies at the opposite
                    // angle. Whether the arc passes that angle, going from `start` by `sweep`:
                    let toward = if radius < 0.0 {
                        angle + std::f64::consts::PI
                    } else {
                        angle
                    };
                    let turn = (toward - start) * sweep.signum();
                    if turn.rem_euclid(std::f64::consts::TAU) <= sweep.abs() {
                        take(g(center + Vec2::new(toward.cos(), toward.sin()) * radius)?);
                    }
                }
            }
            _ => {
                for span in seg.spans()? {
                    let (lo, hi) = span_extremes(span, &g)?;
                    take(lo);
                    take(hi);
                }
            }
        }
    }
    range
}

/// The least and the most of `g` over one homogeneous Bézier span, as values `g` takes at
/// points of it, found by halving ([`affine_extremes`]).
fn span_extremes(span: Vec<H2>, g: &impl Fn(Point2) -> Option<f64>) -> Option<(f64, f64)> {
    let ends = |c: &[H2]| -> Option<(f64, f64)> {
        let (a, b) = (g(point_of(*c.first()?))?, g(point_of(*c.last()?))?);
        Some((a.min(b), a.max(b)))
    };
    let (mut least, mut most) = ends(&span)?;
    let mut todo = vec![(span, 0u32)];
    while let Some((c, depth)) = todo.pop() {
        if c.iter()
            .any(|q| !(q[2] > 0.0 && q.iter().all(|x| x.is_finite())))
        {
            return None;
        }
        let (lo, hi) = ends(&c)?;
        least = least.min(lo);
        most = most.max(hi);
        let values = c
            .iter()
            .map(|q| g(point_of(*q)))
            .collect::<Option<Vec<_>>>()?;
        let (hull_lo, hull_hi) = values
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
                (l.min(*v), h.max(*v))
            });
        let hair = HAIR * (1.0 + hull_lo.abs().max(hull_hi.abs()));
        if (hull_lo >= least - hair && hull_hi <= most + hair) || depth >= MAX_HALVINGS {
            continue;
        }
        let (l, r) = halves(&c);
        todo.push((l, depth + 1));
        todo.push((r, depth + 1));
    }
    Some((least, most))
}
