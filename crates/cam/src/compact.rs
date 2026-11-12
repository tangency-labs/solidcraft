//! Shorter programs for the same cuts, and feeds that hold once the program is written.
//!
//! [`compact`] merges a run of straight cuts at one feed into fewer moves: a run whose points
//! all lie within a tolerance of one straight line becomes that line, and, where asked, a run
//! whose points lie within it of one circular arc in the XY plane (Z level along it, or rising
//! or falling evenly with the angle: a helix) becomes that arc. Every point a merged move
//! replaces is within the tolerance of it. A line is then within the tolerance of the moves it
//! replaces all along them; an arc also bows out from them between their points, by its
//! sagitta over each, and is taken only where that is no more than [`Compaction::bow`]: as far
//! as the path was allowed to cut across a curve when it was planned, so that an arc through
//! points sampled that closely from a curve is taken as that curve, but one through the
//! corners of a polygon (which lie on a circle too) is not.
//!
//! [`limit_descents`] works out again, from the coordinates and feeds as the program writes
//! them (rounded to the post's digits), the feed of every cut that was slowed so the tool goes
//! down no faster than the plunge feed: rounding can make a short move steeper than planned,
//! and the feed is rounded down, never up, so the written program keeps the limit.

use std::f64::consts::PI;

use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::gcode::number;
use crate::path::{MAX_MOVES, Move, Toolpath, arc_sweep};
use crate::{CamError, Result, invalid};

/// Most points one merged move is grown over, to bound the work of a long run.
const MAX_RUN: usize = 400;
/// Fewest moves an arc replaces.
const MIN_ARC_MOVES: usize = 3;
/// Largest radius of a fitted arc (mm): flatter than that is a line.
const MAX_ARC_RADIUS: f64 = 1000.0;
/// Largest sweep of a fitted arc (radians), well short of a full turn.
const MAX_ARC_SWEEP: f64 = PI;

/// How [`compact`] may merge moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compaction {
    /// Farthest any replaced point may be from the move that replaces it (mm).
    pub tolerance: f64,
    /// Whether arcs in the XY plane may be fitted.
    pub arcs: bool,
    /// Whether a fitted arc may change Z along it (a helix).
    pub helical: bool,
    /// Farthest a fitted arc may bow out between two of the points it replaces from the
    /// straight move joining them (mm): its sagitta there.
    pub bow: f64,
}

fn same_feed(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// The circle through three points seen from above: its centre, or `None` when they are in
/// a line.
fn circle(a: Vec2, b: Vec2, c: Vec2) -> Option<Vec2> {
    let (ab, ac) = (b - a, c - a);
    let d = 2.0 * ab.cross(ac);
    if !(d.abs() > 1e-12 * ab.len2().max(ac.len2()).max(1e-300)) {
        return None;
    }
    let (l1, l2) = (ab.len2(), ac.len2());
    let centre = a + Vec2::new(ac.y * l1 - ab.y * l2, ab.x * l2 - ac.x * l1) / d;
    centre.is_finite().then_some(centre)
}

fn flat(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.y)
}

/// Whether the points `pts[1..last]` are within `tol` of the straight line from `pts[0]` to
/// `pts[last]`, in order along it.
fn fits_line(pts: &[Vec3], last: usize, tol: f64) -> bool {
    let (Some(&a), Some(&b)) = (pts.first(), pts.get(last)) else { return false };
    let d = b - a;
    let len2 = d.len2();
    if !(len2 > 1e-24) {
        return false;
    }
    let mut prev_u = 0.0;
    for p in pts.iter().take(last).skip(1) {
        let u = (*p - a).dot(d) / len2;
        if !(u >= prev_u - 1e-12) || u > 1.0 + 1e-12 || a.lerp(b, u.clamp(0.0, 1.0)).dist(*p) > tol {
            return false;
        }
        prev_u = u;
    }
    true
}

/// The arc from `pts[0]` to `pts[last]` through the points between, when they are within
/// `tol` of it (seen from above and in Z, which changes evenly with the angle) and it bows out
/// from the straight moves between them by no more than `bow`: its centre and direction
/// (counter-clockwise when `true`).
fn fits_arc(pts: &[Vec3], last: usize, tol: f64, bow: f64, helical: bool) -> Option<(Vec2, bool)> {
    if last < MIN_ARC_MOVES {
        return None;
    }
    let (a, m, b) = (*pts.first()?, *pts.get(last / 2)?, *pts.get(last)?);
    let c = circle(flat(a), flat(m), flat(b))?;
    let r = flat(a).dist(c);
    if !(r > 10.0 * tol) || !(r <= MAX_ARC_RADIUS) {
        return None;
    }
    let ccw = (flat(m) - flat(a)).cross(flat(b) - flat(m)) > 0.0;
    let sweep = arc_sweep(a, b, c, ccw);
    if !(sweep.abs() <= MAX_ARC_SWEEP) || !(sweep.abs() > 1e-9) {
        return None;
    }
    if !helical && (b.z - a.z).abs() > 1e-12 {
        return None;
    }
    let start = (flat(a) - c).angle();
    // The share of the sweep the point before is at.
    let mut prev = 0.0;
    // Between two points the arc bows out from the chord joining its points there by its
    // sagitta, and the move between them is no farther from that chord than its ends are from
    // the arc: so within the tolerance and the bow of the arc all along.
    let bows = |from: f64, to: f64| r * (1.0 - (0.5 * sweep.abs() * (to - from)).cos()) <= bow;
    for p in pts.iter().take(last).skip(1) {
        // How far round the arc the point is, as a share of the sweep.
        let s = arc_sweep(a, *p, c, ccw);
        let u = if s.abs() <= 1e-12 { 0.0 } else { s / sweep };
        if !(u >= prev - 1e-12) || u > 1.0 + 1e-12 {
            return None;
        }
        let on = c + Vec2::from_angle(start + sweep * u) * r;
        let z = a.z + (b.z - a.z) * u;
        if !(on.dist(flat(*p)).hypot(p.z - z) <= tol) || !bows(prev, u) {
            return None;
        }
        prev = u;
    }
    bows(prev, 1.0).then_some((c, ccw))
}

/// The moves of `run` (straight cuts from `from`, each with its feed) merged within `how`.
fn merge_run(from: Vec3, run: &[(Vec3, f64)], how: &Compaction, out: &mut Vec<Move>) {
    let mut pts: Vec<Vec3> = Vec::with_capacity(run.len() + 1);
    pts.push(from);
    pts.extend(run.iter().map(|(p, _)| *p));
    let mut i = 0;
    while i + 1 < pts.len() {
        let rest = pts.get(i..).unwrap_or(&[]);
        let most = (rest.len() - 1).min(MAX_RUN);
        // The farthest point a line reaches, then the farthest an arc does.
        let mut line_to = 1;
        while line_to < most && fits_line(rest, line_to + 1, how.tolerance) {
            line_to += 1;
        }
        let mut arc: Option<(usize, Vec2, bool)> = None;
        if how.arcs {
            let mut k = MIN_ARC_MOVES;
            while k <= most {
                match fits_arc(rest, k, how.tolerance, how.bow, how.helical) {
                    Some((c, ccw)) => arc = Some((k, c, ccw)),
                    None => break,
                }
                k += 1;
            }
        }
        let feed_of = |a: usize, b: usize| run.get(i + a..i + b).unwrap_or(&[]).iter().map(|(_, f)| *f).fold(f64::INFINITY, f64::min);
        match arc {
            Some((k, center, ccw)) if k > line_to => {
                if let Some(&to) = rest.get(k) {
                    out.push(Move::Arc { to, center, ccw, feed: feed_of(0, k) });
                }
                i += k;
            }
            _ => {
                if let Some(&to) = rest.get(line_to) {
                    out.push(Move::Feed { to, feed: feed_of(0, line_to) });
                }
                i += line_to;
            }
        }
    }
}

/// `path` with its runs of straight cuts at one feed merged into fewer moves (see the module
/// documentation). Rapids, arcs and dwells are kept as they are.
pub fn compact(path: &Toolpath, how: &Compaction) -> Result<Toolpath> {
    if !(how.tolerance >= 0.0) || !(how.tolerance <= 10.0) || !(how.bow >= 0.0) || !(how.bow <= 10.0) {
        return invalid("the compaction tolerance and an arc's bow must be from 0 to 10 mm");
    }
    if path.moves.len() > MAX_MOVES {
        return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
    }
    let mut out: Vec<Move> = Vec::with_capacity(path.moves.len());
    let mut pos: Option<Vec3> = None;
    // The run of straight cuts being gathered: where it starts and its moves.
    let mut run: Vec<(Vec3, f64)> = Vec::new();
    let mut run_from: Option<Vec3> = None;
    let flush = |run: &mut Vec<(Vec3, f64)>, from: Option<Vec3>, out: &mut Vec<Move>| {
        if let Some(f) = from {
            merge_run(f, run, how, out);
        }
        run.clear();
    };
    for m in &path.moves {
        match (*m, pos) {
            (Move::Feed { to, feed }, Some(p)) if to.is_finite() && feed.is_finite() => {
                if run.last().is_some_and(|(_, f)| !same_feed(*f, feed)) {
                    flush(&mut run, run_from, &mut out);
                }
                if run.is_empty() {
                    run_from = Some(p);
                }
                if p.dist(to) > 1e-12 {
                    run.push((to, feed));
                }
            }
            _ => {
                flush(&mut run, run_from, &mut out);
                out.push(*m);
            }
        }
        if let Some(t) = m.target() {
            pos = Some(t);
        }
    }
    flush(&mut run, run_from, &mut out);
    Ok(Toolpath { moves: out })
}

/// `v` mm as the post writes it with `decimals` decimals in units of `unit` mm, back in mm (the
/// writer's own rounding, which takes a half to the even digit).
fn written(v: f64, decimals: u8, unit: f64) -> f64 {
    number(v / unit, decimals).parse::<f64>().map_or(v, |w| w * unit)
}

/// Sets the feed of every cut of `path` slowed below the cut feed `cut` (and going down) so
/// that, between its ends as written (each coordinate with `decimals` decimals, in units of
/// `unit` mm), the tool goes down no faster than `plunge`, the feed rounded down to a whole
/// number of `feed_step` mm/min (never below one step). Cuts at the cut feed are left as
/// planned.
pub fn limit_descents(path: &mut Toolpath, plunge: f64, cut: f64, decimals: u8, unit: f64, feed_step: f64) -> Result<()> {
    if !(plunge > 0.0) || !(cut > 0.0) || !plunge.is_finite() || !cut.is_finite() {
        return invalid("the feeds must be above 0");
    }
    if !(unit > 0.0) || !(feed_step >= 0.0) || !unit.is_finite() || !feed_step.is_finite() || decimals > 12 {
        return invalid("the units must be above 0, the feed step 0 or more, and at most 12 decimals");
    }
    let at = |p: Vec3| Vec3::new(written(p.x, decimals, unit), written(p.y, decimals, unit), written(p.z, decimals, unit));
    let mut pos: Option<Vec3> = None;
    for m in &mut path.moves {
        let target = m.target();
        if let Some(from) = pos {
            let a = at(from);
            let cut_move = match m {
                Move::Feed { to, feed } => Some((a.dist(at(*to)), at(*to), feed)),
                Move::Arc { to, center, ccw, feed } => {
                    let b = at(*to);
                    let r = flat(a).dist(*center);
                    Some(((r * arc_sweep(a, b, *center, *ccw)).hypot(b.z - a.z), b, feed))
                }
                Move::PlaneArc { to, center, plane, ccw, feed } => {
                    let b = at(*to);
                    Some((crate::path::PlaneArc { from: a, to: b, center: *center, plane: *plane, ccw: *ccw }.length(), b, feed))
                }
                _ => None,
            };
            let Some((len, b, feed)) = cut_move else {
                if let Some(t) = target {
                    pos = Some(t);
                }
                continue;
            };
            let drop = a.z - b.z;
            if *feed > 0.0 && *feed < cut && drop > 0.0 && len > 0.0 {
                let limit = (plunge * len / drop).min(*feed);
                let mut f = if feed_step > 0.0 { (limit / feed_step).floor() * feed_step } else { limit };
                if !(f > 0.0) {
                    f = if feed_step > 0.0 { feed_step.min(*feed) } else { *feed };
                }
                *feed = f;
            }
        }
        if let Some(t) = target {
            pos = Some(t);
        }
    }
    Ok(())
}
