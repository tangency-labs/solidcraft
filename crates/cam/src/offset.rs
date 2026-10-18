//! Offsetting of planar regions bounded by lines and arcs (see the crate documentation for
//! the method). Loops keep the region on their left: outer loops run counter-clockwise, holes
//! clockwise. A positive distance moves the boundary into the region (the region shrinks), a
//! negative one away from it.

use std::f64::consts::{FRAC_PI_2, TAU};

use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};

use crate::piece::{Piece, turn};
use crate::{CamError, Result, invalid};

/// Most boundary pieces of one operation's regions (curves count by their line approximation).
pub(crate) const MAX_PIECES: usize = 4000;
/// Most pieces after cutting the offset curves where they cross.
const MAX_SPLIT_PIECES: usize = 200_000;
/// How far apart consecutive profile segments may start and end (mm).
const CLOSED_TOL: f64 = 1e-6;
/// A point this much closer to the boundary than the offset distance still counts as clear (mm).
const CLEAR_TOL: f64 = 1e-9;

/// The boundary of one or more regions.
#[derive(Clone, Debug)]
pub(crate) struct Shape {
    loops: Vec<Vec<Piece>>,
}

impl Shape {
    /// `tol`: chord error allowed when a spline or conic boundary is replaced by lines.
    pub fn from_regions(regions: &[Region2], tol: f64) -> Result<Shape> {
        if !(tol > 0.0) || !tol.is_finite() {
            return invalid("the tolerance must be above 0");
        }
        let mut loops = Vec::new();
        let mut count = 0usize;
        for region in regions {
            for (lp, ccw) in std::iter::once((&region.outer, true)).chain(region.holes.iter().map(|h| (h, false))) {
                let mut pieces = loop_pieces(lp, tol)?;
                count += pieces.len();
                if count > MAX_PIECES {
                    return Err(CamError::TooLarge(format!("the profiles have more than {MAX_PIECES} segments")));
                }
                let area: f64 = pieces.iter().map(Piece::area_term).sum();
                if !(area.abs() > 1e-9) {
                    return invalid("a profile encloses no area");
                }
                if (area > 0.0) != ccw {
                    pieces = reversed(&pieces);
                }
                loops.push(pieces);
            }
        }
        if loops.is_empty() {
            return invalid("there is no profile to machine");
        }
        Ok(Shape { loops })
    }

    pub fn loops(&self) -> &[Vec<Piece>] {
        &self.loops
    }

    fn pieces(&self) -> impl Iterator<Item = &Piece> {
        self.loops.iter().flatten()
    }

    /// Distance from `p` to the boundary.
    pub fn dist(&self, p: Vec2) -> f64 {
        self.pieces().map(|piece| piece.dist(p)).fold(f64::INFINITY, f64::min)
    }

    /// Whether `p` is inside the regions. `sag`: how far `p` is known to be from the boundary
    /// (arcs are walked in chords that stay within a quarter of it).
    pub fn inside(&self, p: Vec2, sag: f64) -> bool {
        let mut total = 0.0;
        for piece in self.pieces() {
            match *piece {
                Piece::Line { a, b } => total += turn(a - p, b - p),
                Piece::Arc { r, sweep, .. } => {
                    let n = arc_steps(r, sweep, sag * 0.25);
                    let mut prev = piece.start();
                    for i in 1..=n {
                        let q = piece.point_at(i as f64 / n as f64);
                        total += turn(prev - p, q - p);
                        prev = q;
                    }
                }
            }
        }
        (total / TAU).round() != 0.0
    }

    /// Whether a tool of radius `r` centred on `p` stays inside the regions without cutting the boundary.
    pub fn clear(&self, p: Vec2, r: f64) -> bool {
        self.dist(p) >= r - 1e-7 && self.inside(p, r.max(1e-6))
    }

    /// The boundary moved by `d` (see the module documentation), as closed loops.
    pub fn offset(&self, d: f64) -> Result<Vec<Vec<Piece>>> {
        if !d.is_finite() {
            return invalid("the offset distance must be finite");
        }
        let ad = d.abs();
        if !(ad > 1e-9) {
            return Ok(self.loops.clone());
        }
        // Crossings closer than this to a piece's end are not cut: the leftover would be too
        // short to tell from a valid piece (see `CLEAR_TOL`), and the gap it leaves is closed
        // when the pieces are joined.
        let min_len = 2.0 * (ad * CLEAR_TOL).sqrt() + 1e-6;
        let mut raw = Vec::new();
        for lp in &self.loops {
            for (piece, next) in lp.iter().zip(lp.iter().cycle().skip(1)) {
                match *piece {
                    Piece::Line { a, b } => {
                        let n = piece.tangent_at(0.0).perp() * d;
                        raw.push(Piece::Line { a: a + n, b: b + n });
                    }
                    Piece::Arc { c, r, a0, sweep } => {
                        let r = r - d * sweep.signum();
                        if r > 1e-9 {
                            raw.push(Piece::Arc { c, r, a0, sweep });
                        }
                    }
                }
                // A corner that opens away from the offset side: the offset rolls round the vertex.
                let dir = piece.tangent_at(1.0);
                let bend = turn(dir, next.tangent_at(0.0));
                if bend * d < 0.0 && bend.abs() > 1e-9 {
                    raw.push(Piece::Arc { c: piece.end(), r: ad, a0: (dir.perp() * d.signum()).angle(), sweep: bend });
                }
            }
        }

        let boxes: Vec<(Vec2, Vec2)> = raw.iter().map(Piece::bounds).collect();
        let mut cuts: Vec<Vec<f64>> = vec![Vec::new(); raw.len()];
        let mut hits = Vec::new();
        for (i, (a, (amin, amax))) in raw.iter().zip(&boxes).enumerate() {
            for (j, (b, (bmin, bmax))) in raw.iter().zip(&boxes).enumerate().skip(i + 1) {
                if amin.x > bmax.x + 1e-9 || bmin.x > amax.x + 1e-9 || amin.y > bmax.y + 1e-9 || bmin.y > amax.y + 1e-9 {
                    continue;
                }
                hits.clear();
                a.intersections(b, &mut hits);
                for &(t, u) in &hits {
                    if let Some(c) = cuts.get_mut(i) {
                        c.push(t);
                    }
                    if let Some(c) = cuts.get_mut(j) {
                        c.push(u);
                    }
                }
            }
        }

        let mut kept = Vec::new();
        let mut total = 0usize;
        for (piece, ts) in raw.iter().zip(cuts.iter_mut()) {
            let len = piece.length();
            ts.retain(|t| t * len > min_len && (1.0 - t) * len > min_len);
            ts.sort_by(f64::total_cmp);
            let mut t0 = 0.0;
            for &t in ts.iter().chain(std::iter::once(&1.0)) {
                if t < 1.0 && !((t - t0) * len > min_len) {
                    continue;
                }
                total += 1;
                if total > MAX_SPLIT_PIECES {
                    return Err(CamError::TooLarge("the offset profile crosses itself too often".into()));
                }
                let sub = piece.sub(t0, t);
                let mid = sub.point_at(0.5);
                if self.dist(mid) >= ad - CLEAR_TOL && self.inside(mid, ad) == (d > 0.0) {
                    kept.push(sub);
                }
                t0 = t;
            }
        }

        let join_tol = 4.0 * min_len;
        Ok(stitch(&kept, join_tol)
            .into_iter()
            .filter(|lp| {
                let (area, length) = lp.iter().fold((0.0, 0.0), |(a, l), p| (a + p.area_term(), l + p.length()));
                area.abs() > join_tol * length
            })
            .collect())
    }
}

/// How many chords walk an arc with at most `sag` between chord and arc.
fn arc_steps(r: f64, sweep: f64, sag: f64) -> usize {
    let step = if sag > 0.0 && sag < r { 2.0 * (1.0 - sag / r).acos() } else { FRAC_PI_2 };
    let n = (sweep.abs() / step.clamp(1e-3, FRAC_PI_2)).ceil();
    if n.is_finite() { (n as usize).clamp(1, 8192) } else { 1 }
}

pub(crate) fn reversed(lp: &[Piece]) -> Vec<Piece> {
    lp.iter().rev().map(Piece::reversed).collect()
}

/// A profile loop as lines and arcs (its other curves as lines within `tol`).
fn loop_pieces(lp: &Loop2, tol: f64) -> Result<Vec<Piece>> {
    let mut out = Vec::new();
    for seg in &lp.segs {
        match *seg {
            Seg2::Line { a, b } => {
                if a.dist(b) > 1e-9 {
                    out.push(Piece::Line { a, b });
                }
            }
            Seg2::Arc { center, radius, start, sweep } => {
                if radius > 1e-9 && sweep.abs() > 1e-12 {
                    out.push(Piece::Arc { c: center, r: radius, a0: start, sweep });
                }
            }
            _ => {
                for w in seg.polyline(tol).windows(2) {
                    if let [a, b] = *w
                        && a.dist(b) > 1e-9
                    {
                        out.push(Piece::Line { a, b });
                    }
                }
            }
        }
        if out.len() > MAX_PIECES {
            return Err(CamError::TooLarge(format!("a profile has more than {MAX_PIECES} segments")));
        }
    }
    if out.is_empty() {
        return invalid("a profile is empty");
    }
    if !out.iter().all(Piece::is_finite) {
        return invalid("a profile has a coordinate that is not a finite number");
    }
    if out.iter().zip(out.iter().cycle().skip(1)).any(|(a, b)| a.end().dist(b.start()) > CLOSED_TOL) {
        return invalid("a profile is not closed");
    }
    Ok(out)
}

/// Joins pieces end to start into closed loops. Where several pieces leave a point the
/// left-most one is taken: the kept side of every piece is on its left, so the boundary of
/// what is kept turns left there. Chains that do not close are dropped.
fn stitch(pieces: &[Piece], tol: f64) -> Vec<Vec<Piece>> {
    let mut used = vec![false; pieces.len()];
    let mut loops = Vec::new();
    for (s, first) in pieces.iter().enumerate() {
        if std::mem::replace(used.get_mut(s).unwrap_or(&mut true), true) {
            continue;
        }
        let origin = first.start();
        let mut chain = vec![*first];
        let mut closed = false;
        for _ in 0..pieces.len() {
            let Some(last) = chain.last() else { break };
            let (end, dir) = (last.end(), last.tangent_at(1.0));
            if end.dist(origin) <= tol && (chain.len() > 1 || last.length() > 2.0 * tol) {
                closed = true;
                break;
            }
            let mut best: Option<(usize, f64)> = None;
            for (j, (c, taken)) in pieces.iter().zip(&used).enumerate() {
                if *taken || c.start().dist(end) > tol {
                    continue;
                }
                let bend = turn(dir, c.tangent_at(0.0));
                if best.is_none_or(|(_, b)| bend > b) {
                    best = Some((j, bend));
                }
            }
            let Some((j, _)) = best else { break };
            if let (Some(flag), Some(piece)) = (used.get_mut(j), pieces.get(j)) {
                *flag = true;
                chain.push(*piece);
            }
        }
        if closed {
            loops.push(chain);
        }
    }
    loops
}

/// The boundaries of `regions` moved by `distance` (positive: into the regions, so they
/// shrink; negative: outwards), as closed loops with what is left on their left. Lines and arcs
/// stay exact; other curves are followed within `tol`.
pub fn offset_regions(regions: &[Region2], distance: f64, tol: f64) -> Result<Vec<Loop2>> {
    let shape = Shape::from_regions(regions, tol)?;
    Ok(shape.offset(distance)?.iter().map(|lp| Loop2 { segs: lp.iter().map(|p| p.seg()).collect() }).collect())
}
