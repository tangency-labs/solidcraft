//! The folded sheet of a surface model (#6748): its solid, sewn from the pieces of its flat
//! pattern, each carried into space through the sheet's thickness by the model's fold. The flat and the solid
//! are made of the same pieces, so they can't disagree.
//!
//! Each piece gives a face on either side of the sheet (its surface, and the face its thickness
//! away) and a wall along each of its edges that no other piece shares. A flat edge folds to a
//! line or a circular arc where it lies along or across a bend or a roll, and to a chain of
//! Bézier pieces of degree 7 elsewhere, such as a slanted edge across a bend, which folds to a
//! helix.

use solidcraft_geom::prelude::*;
use std::sync::Arc;

use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_develop::{Bent, Development, Fold};
use solidcraft_geom::LINEAR_TOL;
use solidcraft_kernel::{self as kernel, Body, EdgeSpec, FaceSpec, SurfSpec};

use crate::sheet::FaceRole;

use crate::sheet::FlatWork;
use crate::sheet_surface::Piece;
use crate::{DocError, Result};

/// How many times along a folded edge it is sampled, to tell a line or an arc.
const SAMPLES: usize = 16;
/// How far (mm) a folded edge's points may lie from the line or the arc it is taken for.
const ON_CURVE: f64 = 1e-9;
/// How far an arc of an edge may lie off the plane of the arc a thickness away and the wall
/// between them still be that plane (a bend's or a roll's end). Folding a part 100 mm across
/// leaves its points about 1e-9 off where they would be exactly; a wall taken for a cone that near
/// its plane would be a cone with no height (a half angle of 90°), which no kernel takes.
const IN_PLANE: f64 = 1e-7;
/// How far (mm) a chain of pieces may lie from the folded edge it stands in for.
const CHAIN_TOL: f64 = 1e-7;
/// The degree of a chain's pieces. Fitted pieces of degree 7 follow a helix round a whole bend
/// in one or two pieces, where cubic ones took 32 (the distance falls as the eighth power of a
/// piece's length, not the fourth).
const CHAIN_DEGREE: usize = 7;
/// The most pieces of one chain, within the kernel's limit on a chain's control points.
const MAX_CHAIN_PIECES: usize = 256;
/// How many points along each piece it is fitted to; it is checked at the points halfway
/// between them.
const FIT_POINTS: usize = 32;
/// The step (a fraction of a chain's piece) a folded edge's direction is measured over.
const STEP: f64 = 1e-4;

/// A run of a cell's loop folded as one edge, and so walled by one face: a line or an arc on its
/// own, or free-form segments that join with no corner, such as a torn end's spline spans.
type Run = Vec<Seg2>;

/// A loop's runs (see [`Run`]), none wrapping round where the loop starts. A loop that is all
/// one smooth curve is two.
fn runs(segs: &[Seg2]) -> Vec<Run> {
    let free = |s: &Seg2| !matches!(s, Seg2::Line { .. } | Seg2::Arc { .. });
    let smooth = |a: &Seg2, b: &Seg2| free(a) && free(b) && a.end_tangent().dot(b.start_tangent()) > 1.0 - 1e-9;
    let n = segs.len();
    let joins = |i: usize| segs.get((i + n - 1) % n).zip(segs.get(i)).is_some_and(|(a, b)| smooth(a, b));
    let Some(first) = (0..n).find(|i| !joins(*i)) else {
        let (head, tail) = segs.split_at(n / 2);
        return [head, tail].into_iter().filter(|r| !r.is_empty()).map(<[Seg2]>::to_vec).collect();
    };
    let mut out: Vec<Run> = Vec::new();
    for k in 0..n {
        let i = (first + k) % n;
        let Some(seg) = segs.get(i) else { continue };
        match out.last_mut() {
            Some(run) if k > 0 && joins(i) => run.push(*seg),
            _ => out.push(vec![*seg]),
        }
    }
    out
}

/// The point of a run `s` along it, from its start (0) to its end (1), each of its segments an
/// equal share.
fn run_at(run: &[Seg2], s: f64) -> Vec2 {
    let n = run.len().max(1);
    let x = (s * n as f64).clamp(0.0, n as f64);
    let i = (x.floor() as usize).min(n - 1);
    run.get(i).map_or(Vec2::ZERO, |seg| seg.point_at(x - i as f64))
}

/// A piece of the flat, its loops' edges split where other pieces' corners touch them.
struct Cell {
    piece: Piece,
    outer: Vec<Seg2>,
    holes: Vec<Vec<Seg2>>,
}

impl Cell {
    fn loops(&self) -> impl Iterator<Item = &Vec<Seg2>> {
        std::iter::once(&self.outer).chain(&self.holes)
    }
}

/// A development folded, `t` thick.
struct Folded<'a> {
    dev: &'a Development,
    fold: &'a Fold,
    t: f64,
}

/// The folded solid of a developed sheet `t` thick: each of its flat's `pieces` (see
/// [`crate::sheet_surface::pieces`]), carried into space by `fold`; and the name `name` gives each
/// face of it, by the piece and the part of it the face was made for.
pub(crate) fn solid(
    dev: &Development,
    fold: &Fold,
    t: f64,
    pieces: &[(Piece, Region2)],
    work: &mut FlatWork,
    name: &dyn Fn(Piece, FaceRole) -> Option<String>,
) -> Result<(Body, Vec<Option<String>>)> {
    let f = Folded { dev, fold, t };
    let cells = cells(pieces.to_vec(), work)?;
    let near = |p: Vec2, q: Vec2| p.dist(q) < 1e-6;
    // Shared with a neighbor: the same flat line the other way, landing in the same place
    // (flat neighbors from two pieces laid side by side do not).
    let shared = |ci: usize, me: &Cell, e: &Seg2| -> Result<bool> {
        let here = f.at(me.piece, e.point_at(0.5), 0.0)?;
        for (cj, c) in cells.iter().enumerate() {
            if cj == ci {
                continue;
            }
            for g in c.loops().flatten() {
                let twin = near(g.start(), e.end()) && near(g.end(), e.start()) && matches!((g, e), (Seg2::Line { .. }, Seg2::Line { .. }));
                if twin && f.at(c.piece, g.point_at(0.5), 0.0)?.dist(here) < 1e-6 {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    };
    let mut faces = Vec::new();
    let mut names = Vec::new();
    for (ci, c) in cells.iter().enumerate() {
        // Each run at both depths, once.
        let runs: Vec<Vec<Run>> = c.loops().map(|l| runs(l)).collect();
        let edges: Vec<Vec<[EdgeSpec; 2]>> =
            runs.iter().map(|l| l.iter().map(|r| f.edges(c.piece, r)).collect::<Result<_>>()).collect::<Result<_>>()?;
        let samples: Vec<Vec2> = c.loops().flatten().flat_map(|e| (0..SAMPLES).map(|k| e.point_at(k as f64 / SAMPLES as f64))).collect();
        let curved = c.loops().flatten().any(|e| !matches!(e, Seg2::Line { .. }));
        let center = c.outer.iter().map(Seg2::start).fold(Vec2::ZERO, |a, b| a + b) * (1.0 / c.outer.len().max(1) as f64);
        // The face a thickness into the material (its loops as the flat's, about the normal
        // toward the material), and the surface (the other way round).
        for (side, depth, inside) in [(1, t, t * (1.0 - 1e-3)), (0, 0.0, t * 1e-3)] {
            let loops = edges
                .iter()
                .map(|l| {
                    let v = l.iter().filter_map(|pair| pair.get(side).cloned());
                    if side == 0 { v.rev().map(|e| e.reversed()).collect() } else { v.collect() }
                })
                .collect();
            let p = f.at(c.piece, center, depth)?;
            faces.push(FaceSpec {
                loops,
                surface: f.surface(c.piece, &samples, depth, curved)?,
                probe: Some((p, p - f.at(c.piece, center, inside)?)),
            });
            names.push(name(c.piece, FaceRole::Cap { surface: side == 0 }));
        }
        // Walls along runs no other cell shares.
        for (run, [bottom, top]) in runs.iter().flatten().zip(edges.iter().flatten()) {
            if let [e] = run.as_slice()
                && shared(ci, c, e)?
            {
                continue;
            }
            let (a, b) = (run_at(run, 0.0), run_at(run, 1.0));
            let up_b = EdgeSpec::Line { a: f.at(c.piece, b, 0.0)?, b: f.at(c.piece, b, t)? };
            let down_a = EdgeSpec::Line { a: f.at(c.piece, a, t)?, b: f.at(c.piece, a, 0.0)? };
            let surface = wall(bottom, top)?;
            let mid = run_at(run, 0.5);
            let along = run_at(run, 0.51) - run_at(run, 0.49);
            let pm = f.at(c.piece, mid, t / 2.0)?;
            let outward = f.at(c.piece, mid + Vec2::new(along.y, -along.x).normalized().unwrap_or_default() * 1e-3, t / 2.0)? - pm;
            faces.push(FaceSpec { loops: vec![vec![bottom.clone(), up_b, top.reversed(), down_a]], surface, probe: Some((pm, outward)) });
            names.push(name(c.piece, FaceRole::Wall { a, b }));
        }
    }
    // Until the kernel sews faces that touch along an edge face to face (#1597): two walls on one
    // edge are refused as "ambiguous edge contact", so no edge may get a wall from both sides.
    let (body, from) = kernel::sew_touching_traced(&faces)?;
    let names = match from {
        Some(from) => from.iter().map(|&k| names.get(k).cloned().flatten()).collect(),
        None => vec![None; body.face_count()],
    };
    Ok((body, names))
}

/// The pieces as cells: edges shorter than the model's tolerance dropped (the sew welds their
/// ends), arcs of more than three quarters of a turn halved (so that an arc's ends stay well
/// apart, as the kernel's own profiles keep them), and straight edges split where other cells'
/// corners touch them, so that neighbors share whole edges.
///
/// Until the kernel (solidcore, #1597) takes a circular edge round any sweep, a whole circle on
/// one vertex too, and sews edges that share only part of their length: keep every edge whole.
/// Both splits here are the kernel's limits, not the sheet's: each makes one edge of the flat
/// two edges of the solid.
fn cells(pieces: Vec<(Piece, Region2)>, work: &mut FlatWork) -> Result<Vec<Cell>> {
    let clean = |l: Loop2| -> Vec<Seg2> {
        let mut v = Vec::with_capacity(l.segs.len());
        for e in l.segs.into_iter().filter(|e| e.length() >= LINEAR_TOL) {
            match e {
                Seg2::Arc { center, radius, start, sweep } if sweep.abs() > std::f64::consts::PI * 1.5 => {
                    let half = sweep / 2.0;
                    v.push(Seg2::Arc { center, radius, start, sweep: half });
                    v.push(Seg2::Arc { center, radius, start: start + half, sweep: half });
                }
                _ => v.push(e),
            }
        }
        v
    };
    let raw: Vec<Cell> = pieces
        .into_iter()
        .filter_map(|(piece, r)| {
            let outer = clean(r.outer);
            (!outer.is_empty()).then(|| Cell { piece, outer, holes: r.holes.into_iter().map(clean).filter(|h| !h.is_empty()).collect() })
        })
        .collect();
    let edges = raw.iter().map(|c| c.loops().map(Vec::len).sum::<usize>()).sum::<usize>();
    work.charge(edges.saturating_mul(edges).saturating_mul(4))?;
    let corners: Vec<Vec2> = raw.iter().flat_map(|c| c.loops().flatten().map(Seg2::start)).collect();
    let split = |segs: &[Seg2]| -> Vec<Seg2> {
        let mut v = Vec::with_capacity(segs.len());
        for e in segs {
            let Seg2::Line { a, b } = *e else {
                v.push(*e);
                continue;
            };
            let d = b - a;
            let (l2, l) = (d.len2(), d.len());
            // Not within the model's tolerance of an end or of each other.
            let mut ts: Vec<f64> = corners
                .iter()
                .filter_map(|p| {
                    let t = (*p - a).dot(d) / l2.max(1e-30);
                    (t * l > LINEAR_TOL && (1.0 - t) * l > LINEAR_TOL && (a + d * t).dist(*p) < 1e-7).then_some(t)
                })
                .collect();
            ts.sort_by(|x, y| x.total_cmp(y));
            ts.dedup_by(|x, y| (*x - *y).abs() * l < LINEAR_TOL);
            let mut prev = a;
            for t in ts {
                let q = a + d * t;
                v.push(Seg2::Line { a: prev, b: q });
                prev = q;
            }
            v.push(Seg2::Line { a: prev, b });
        }
        v
    };
    Ok(raw.iter().map(|c| Cell { piece: c.piece, outer: split(&c.outer), holes: c.holes.iter().map(|h| split(h)).collect() }).collect())
}

impl Folded<'_> {
    fn bent(&self, zone: usize) -> Option<&Bent> {
        self.dev.zones.get(zone).and_then(|z| self.fold.bends.get(z.joint)).and_then(Option::as_ref)
    }

    /// Where a point of a piece goes, `depth` into the material: a tab's where its panel puts it,
    /// carried on flat past the panel's edge (along the roll's surface, on a roll).
    fn at(&self, piece: Piece, p: Vec2, depth: f64) -> Result<Vec3> {
        let got = match piece {
            Piece::Panel(i) | Piece::Tab(_, i) => self.fold.panel_point(i, p, depth),
            Piece::Zone(z) => self.bent(z).map(|b| b.point(p, depth)),
        };
        got.filter(|q| q.is_finite()).ok_or_else(|| DocError::Invalid("a piece of the flat pattern has no place on the folded sheet".into()))
    }

    /// A run of a piece's flat edges folded to the surface and to a thickness into the material: a
    /// line or an arc at both where it folds to one, else chains of pieces of [`CHAIN_DEGREE`]
    /// split alike, so that a ruled wall joins them piece by piece.
    fn edges(&self, piece: Piece, run: &[Seg2]) -> Result<[EdgeSpec; 2]> {
        let sampled = |depth: f64| (0..=SAMPLES).map(|k| self.at(piece, run_at(run, k as f64 / SAMPLES as f64), depth)).collect::<Result<Vec<_>>>();
        if let (Some(surface), Some(deep)) = (simple(&sampled(0.0)?), simple(&sampled(self.t)?)) {
            return Ok([surface, deep]);
        }
        // Equal pieces of each of the run's segments, as few as keep within the tolerance at
        // both depths; each joint of the run starts a piece.
        let segs = run.len().max(1);
        let mut each = 1;
        loop {
            let n = segs * each;
            let off = match (self.chain(piece, run, 0.0, n)?, self.chain(piece, run, self.t, n)?) {
                (Some((surface, a)), Some((deep, b))) if a.max(b) <= CHAIN_TOL => return Ok([surface, deep]),
                (Some((_, a)), Some((_, b))) => a.max(b),
                _ => f64::INFINITY,
            };
            if n >= MAX_CHAIN_PIECES {
                return Err(DocError::Invalid("an edge of the folded sheet bends too much to follow".into()));
            }
            // The distance falls as the eighth power of the pieces' length.
            let guess = if off.is_finite() { (each as f64 * (off / CHAIN_TOL).powf(1.0 / 8.0)).ceil() as usize } else { each * 2 };
            each = guess.clamp(each + 1, (MAX_CHAIN_PIECES / segs).max(each + 1));
        }
    }

    /// A folded run `depth` into the material as a chain of `n` equal pieces of [`CHAIN_DEGREE`],
    /// and the farthest it lies from the run between the points it was fitted to; `None` where a
    /// piece could not be fitted. Each piece passes through the run's points at its ends, leaves
    /// and meets them the way the run does there, and lies as near the run as it can (least
    /// squares) at [`FIT_POINTS`] points along it. Neighbours share the way at their joint, so the
    /// chain turns no corner.
    fn chain(&self, piece: Piece, run: &[Seg2], depth: f64, n: usize) -> Result<Option<(EdgeSpec, f64)>> {
        let at = |s: f64| self.at(piece, run_at(run, s), depth);
        let h = STEP / n as f64;
        // The way the run goes at `s` measured from one side (`h` > 0 ahead, < 0 behind), so that
        // none is measured across a corner of the run.
        let rate = |s: f64, h: f64| -> Result<Vec3> { Ok((at(s)? * -3.0 + at(s + h)? * 4.0 - at(s + 2.0 * h)?) * (1.0 / (2.0 * h))) };
        let mut ways = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let s = i as f64 / n as f64;
            let way = match i {
                0 => rate(s, h)?.normalized(),
                _ if i == n => rate(s, -h)?.normalized(),
                // Inside the run it is smooth: both sides agree, but for the run's own speed.
                _ => rate(s, h)?.normalized().zip(rate(s, -h)?.normalized()).and_then(|(a, b)| (a + b).normalized()),
            };
            ways.push(way);
        }
        let d = CHAIN_DEGREE;
        let mut ctrl = Vec::with_capacity(d * n + 1);
        let mut worst = 0.0f64;
        for k in 0..n {
            let (s0, s1) = (k as f64 / n as f64, (k + 1) as f64 / n as f64);
            let (p0, p1) = (at(s0)?, at(s1)?);
            let (Some(Some(w0)), Some(Some(w1))) = (ways.get(k).copied(), ways.get(k + 1).copied()) else {
                return Err(DocError::Invalid("an edge of the folded sheet has no direction".into()));
            };
            let mut fitted = Vec::with_capacity(FIT_POINTS);
            for j in 1..FIT_POINTS {
                let u = j as f64 / FIT_POINTS as f64;
                fitted.push((u, at(s0 + (s1 - s0) * u)?));
            }
            let Some(c) = fit_piece((p0, w0), (p1, w1), &fitted) else { return Ok(None) };
            for j in 0..FIT_POINTS {
                let u = (j as f64 + 0.5) / FIT_POINTS as f64;
                worst = worst.max(bezier(&c, u).dist(at(s0 + (s1 - s0) * u)?));
            }
            ctrl.extend_from_slice(if k == 0 { &c[..] } else { &c[1..] });
        }
        Ok(Some((EdgeSpec::Chain { degree: d, ctrl: Arc::from(ctrl) }, worst)))
    }

    /// The surface of a piece's face `depth` into the material, holding the flat points
    /// `samples` of its boundary: a plane, or the line across a bend or a roll swept round its
    /// axis.
    fn surface(&self, piece: Piece, samples: &[Vec2], depth: f64, curved: bool) -> Result<SurfSpec> {
        let range = |v: &mut dyn Iterator<Item = f64>| v.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let lost = || DocError::Invalid("a piece of the flat pattern has no place on the folded sheet".into());
        match piece {
            Piece::Zone(z) => {
                let b = self.bent(z).ok_or_else(lost)?;
                let w: Vec<(f64, f64)> = samples.iter().map(|p| b.wrapped(*p)).collect();
                let ((mut l0, mut l1), (mut p0, mut p1)) = (range(&mut w.iter().map(|x| x.0)), range(&mut w.iter().map(|x| x.1)));
                // The surface must hold the whole boundary: a curved edge (a torn end, a corner's
                // round) may bulge past its samples, so the surface reaches a little further,
                // never past the bend's own turn.
                if curved {
                    (l0, l1, p0, p1) = (l0 - 1e-3, l1 + 1e-3, (p0 - 1e-4).max(0.0), (p1 + 1e-4).min(b.angle()));
                }
                let (axis_origin, axis) = b.axis();
                Ok(SurfSpec::Revolved { a: b.at(l0, p0, depth), b: b.at(l1, p0, depth), axis_origin, axis, angle: p1 - p0 })
            }
            Piece::Panel(i) | Piece::Tab(_, i) => {
                let Some((axis_origin, axis)) = self.fold.axis(i) else { return Ok(SurfSpec::Plane) };
                let q: Vec<Vec2> = samples.iter().map(|p| self.fold.param(i, *p)).collect::<Option<_>>().ok_or_else(lost)?;
                let ((u0, u1), (v0, v1)) = (range(&mut q.iter().map(|q| q.x)), range(&mut q.iter().map(|q| q.y)));
                let at = |u: f64| self.fold.surface_point(i, Vec2::new(u, v0), depth).ok_or_else(lost);
                Ok(SurfSpec::Revolved { a: at(u0)?, b: at(u1)?, axis_origin, axis, angle: v1 - v0 })
            }
        }
    }
}

/// The line or the circular arc through a folded edge's samples, where they lie on one.
fn simple(pts: &[Vec3]) -> Option<EdgeSpec> {
    let (&a, &b) = (pts.first()?, pts.last()?);
    if pts.iter().all(|p| p.dist_to_segment(a, b) <= ON_CURVE) {
        return Some(EdgeSpec::Line { a, b });
    }
    let mid = *pts.get(pts.len() / 2)?;
    let (c, n) = circle(a, mid, b)?;
    let r = a.dist(c);
    pts.iter().all(|p| (p.dist(c) - r).abs() <= ON_CURVE && (*p - c).dot(n).abs() <= ON_CURVE).then_some(EdgeSpec::Arc { a, b, mid })
}

/// The circle through `a`, `m` and `b`: its center, and the normal about which `a`, `m`, `b`
/// run counter-clockwise.
fn circle(a: Vec3, m: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
    let (u, v) = (m - a, b - a);
    let w = u.cross(v);
    let w2 = w.len2();
    if !(w2 > 1e-24) {
        return None;
    }
    Some((a + (v.cross(w) * u.len2() + w.cross(u) * v.len2()) * (1.0 / (2.0 * w2)), w.normalized()?))
}

/// The point at `u` of the Bézier over `c` (de Casteljau).
fn bezier(c: &[Vec3], u: f64) -> Vec3 {
    let mut w = c.to_vec();
    while w.len() > 1 {
        w = w.windows(2).map(|p| p[0] * (1.0 - u) + p[1] * u).collect();
    }
    w.first().copied().unwrap_or(Vec3::ZERO)
}

/// The Bézier piece of [`CHAIN_DEGREE`] from `p0` to `p1` that leaves `p0` along `w0` and meets
/// `p1` along `w1` (unit ways, its speed there free) and lies nearest the `points` (each with its
/// parameter along the piece) by least squares. `None` where none does, or where the best runs
/// back against either way.
fn fit_piece((p0, w0): (Vec3, Vec3), (p1, w1): (Vec3, Vec3), points: &[(f64, Vec3)]) -> Option<[Vec3; CHAIN_DEGREE + 1]> {
    const D: usize = CHAIN_DEGREE;
    // The unknowns: the speeds at both ends, then the inner control points 2 ..= D - 2, each
    // measured from `p0` so that the sums stay small beside the piece's own size.
    const N: usize = 2 + 3 * (D - 3);
    let axis = |v: Vec3, c: usize| [v.x, v.y, v.z].get(c).copied().unwrap_or(0.0);
    let binomial = |k: usize| (0..k).fold(1.0, |b, i| b * (D - i) as f64 / (i + 1) as f64);
    let (mut m, mut rhs) = ([[0.0f64; N]; N], [0.0f64; N]);
    let end = p1 - p0;
    for &(u, q) in points {
        let b: Vec<f64> = (0..=D).map(|k| binomial(k) * u.powi(k as i32) * (1.0 - u).powi((D - k) as i32)).collect();
        let (b1, b6, b7) = (b.get(1).copied()?, b.get(D - 1).copied()?, b.get(D).copied()?);
        let target = (q - p0) - end * (b6 + b7);
        for c in 0..3 {
            let mut row = [0.0f64; N];
            row[0] = b1 * axis(w0, c);
            row[1] = -b6 * axis(w1, c);
            for i in 2..=D - 2 {
                row[2 + 3 * (i - 2) + c] = b.get(i).copied()?;
            }
            let r = axis(target, c);
            for (x, rx) in row.iter().enumerate() {
                rhs[x] += rx * r;
                for (y, ry) in row.iter().enumerate() {
                    m[x][y] += rx * ry;
                }
            }
        }
    }
    let x = solve(m, rhs)?;
    let (a, b) = (x[0], x[1]);
    if !(a > 0.0 && b > 0.0) {
        return None;
    }
    let mut c = [p0; D + 1];
    c[1] = p0 + w0 * a;
    for (i, ci) in c.iter_mut().enumerate().take(D - 1).skip(2) {
        let k = 2 + 3 * (i - 2);
        *ci = p0 + Vec3::new(x[k], x[k + 1], x[k + 2]);
    }
    c[D - 1] = p1 - w1 * b;
    c[D] = p1;
    Some(c)
}

/// The solution of `m x = rhs` (Gaussian elimination with partial pivoting); `None` where `m` is
/// singular.
fn solve<const N: usize>(mut m: [[f64; N]; N], mut rhs: [f64; N]) -> Option<[f64; N]> {
    for col in 0..N {
        let pivot = (col..N).max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))?;
        if !(m[pivot][col].abs() > 1e-300) {
            return None;
        }
        m.swap(col, pivot);
        rhs.swap(col, pivot);
        for row in col + 1..N {
            let f = m[row][col] / m[col][col];
            let (above, below) = m.split_at_mut(row);
            for (a, p) in below[0][col..].iter_mut().zip(&above[col][col..]) {
                *a -= f * p;
            }
            rhs[row] -= f * rhs[col];
        }
    }
    let mut x = [0.0f64; N];
    for row in (0..N).rev() {
        let tail: f64 = (row + 1..N).map(|k| m[row][k] * x[k]).sum();
        x[row] = (rhs[row] - tail) / m[row][row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// The surface of the wall between an edge's folds on the surface (`bottom`) and a thickness in
/// (`top`), both running the same way.
fn wall(bottom: &EdgeSpec, top: &EdgeSpec) -> Result<SurfSpec> {
    match (bottom, top) {
        (EdgeSpec::Line { .. }, EdgeSpec::Line { .. }) => Ok(SurfSpec::Plane),
        (&EdgeSpec::Arc { a, b, mid }, &EdgeSpec::Arc { a: ta, b: tb, mid: tm }) => {
            let (c, n) = circle(a, mid, b).ok_or_else(|| DocError::Invalid("an arc of the folded sheet has no circle".into()))?;
            // In one plane: a bend's or a roll's end.
            if [ta, tb, tm].iter().all(|p| (*p - c).dot(n).abs() <= IN_PLANE) {
                return Ok(SurfSpec::Plane);
            }
            // Moved square off its panel: a round edge of a planar panel.
            let d = ta - a;
            if (tb - b - d).len() <= ON_CURVE && (tm - mid - d).len() <= ON_CURVE {
                return Ok(SurfSpec::Extruded { a, b, mid, dir: d });
            }
            // Round the same axis: a circle round a cone, its wall a cone of its own.
            let (e1, e2) = ((a - c).normalized().unwrap_or(Vec3::X), n.cross((a - c).normalized().unwrap_or(Vec3::X)));
            let angle = (b - c).dot(e2).atan2((b - c).dot(e1)).rem_euclid(std::f64::consts::TAU);
            Ok(SurfSpec::Revolved { a, b: ta, axis_origin: c, axis: n, angle })
        }
        (EdgeSpec::Chain { .. }, EdgeSpec::Chain { .. }) => Ok(SurfSpec::Ruled { a: bottom.clone(), b: top.clone() }),
        _ => Err(DocError::Invalid("not supported yet: an edge that folds to a line on one side of the sheet and a curve on the other".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_develop::{Anchor, Bend, Datum, Joint, JointKind, Panel, Relief, Rigid3, Sheet, Tail, ZoneEnds, develop, fold};

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
        let pts = [Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)];
        Region2 { outer: Loop2::polygon(&pts).ccw(), holes: Vec::new() }
    }

    /// A flat line slanted across the whole zone of a 90° bend (2 mm sheet, inside radius 2,
    /// K 0.5) folds to a helix on either face of the sheet: round the bend's axis at 4 mm on its
    /// surface and 2 mm a thickness in. Two pieces of degree 7 follow each within the tolerance
    /// (cubic ones took 32), and they join without a turn.
    #[test]
    fn a_slanted_edge_across_a_bend_folds_to_two_pieces_of_its_helix() {
        let bend = Bend {
            angle: std::f64::consts::FRAC_PI_2,
            radius: 2.0,
            toward_material: true,
            datum: Datum::Sharp,
            length: 100.0,
            ends: ZoneEnds { near: (0.0, 100.0), far: (0.0, 100.0) },
            tails: (Tail::Keep, Tail::Trim),
            relief: (Relief::None, Relief::None),
        };
        let sheet = Sheet {
            thickness: 2.0,
            k_factor: 0.5,
            panels: vec![
                Panel { name: "base".into(), region: rect(0.0, 0.0, 100.0, 60.0), ..Panel::default() },
                Panel { name: "flange".into(), region: rect(0.0, -20.0, 100.0, 0.0), ..Panel::default() },
            ],
            joints: vec![Joint {
                a: Anchor { panel: 0, origin: Vec2::ZERO, dir: Vec2::X },
                b: Anchor { panel: 1, origin: Vec2::ZERO, dir: Vec2::X },
                kind: JointKind::Bend(bend),
            }],
            corners: Vec::new(),
            cuts: Vec::new(),
        };
        let dev = develop(&sheet, 0).expect("the bracket develops");
        let folded = fold(&sheet, &dev, &[Rigid3::IDENTITY]).expect("the bracket folds");
        let [zone] = dev.zones.as_slice() else { panic!("one zone: {:?}", dev.zones) };
        let line = Seg2::Line { a: zone.near[0] + (zone.near[1] - zone.near[0]) * 0.2, b: zone.far[0] + (zone.far[1] - zone.far[0]) * 0.35 };
        let f = Folded { dev: &dev, fold: &folded, t: 2.0 };
        let (o, w) = f.bent(0).expect("the zone is bent").axis();
        let edges = f.edges(Piece::Zone(0), &[line]).expect("the line folds");
        for (edge, (depth, radius)) in edges.iter().zip([(0.0, 4.0), (2.0, 2.0)]) {
            let EdgeSpec::Chain { degree: 7, ctrl } = edge else { panic!("a slanted edge folds to a chain of degree 7: {edge:?}") };
            let n = (ctrl.len() - 1) / 7;
            assert!(n <= 2, "depth {depth}: {n} pieces");
            for k in 0..n {
                for i in 0..=40 {
                    let x = i as f64 / 40.0;
                    let p = bezier(&ctrl[7 * k..=7 * k + 7], x);
                    let exact = f.at(Piece::Zone(0), line.point_at((k as f64 + x) / n as f64), depth).expect("on the bend");
                    assert!(p.dist(exact) < CHAIN_TOL, "depth {depth}, piece {k}, {x}: {}", p.dist(exact));
                    let off_axis = (p - o) - w * (p - o).dot(w);
                    assert!((off_axis.len() - radius).abs() < CHAIN_TOL, "depth {depth}: radius {}", off_axis.len());
                }
            }
            for k in 1..n {
                let (before, after) = ((ctrl[7 * k] - ctrl[7 * k - 1]).normalized(), (ctrl[7 * k + 1] - ctrl[7 * k]).normalized());
                let turn = before.zip(after).map_or(0.0, |(b, a)| b.dot(a));
                assert!(turn > 1.0 - 1e-12, "depth {depth}: the pieces turn at joint {k}: {turn}");
            }
        }
    }
}
