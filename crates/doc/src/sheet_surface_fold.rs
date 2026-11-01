//! The folded sheet of a surface model (#6748): its solid, sewn from the pieces of its flat
//! pattern, each carried into space through the sheet's thickness by the model's fold. The flat and the solid
//! are made of the same pieces, so they can't disagree.
//!
//! Each piece gives a face on either side of the sheet (its surface, and the face its thickness
//! away) and a wall along each of its edges that no other piece shares. A flat edge folds to a
//! line or a circular arc where it lies along or across a bend or a roll, and to a chain of cubic
//! pieces elsewhere, such as a slanted edge across a bend, which folds to a helix.

use std::sync::Arc;

use solidcraft_develop::{Bent, Development, Fold};
use solidcraft_geom::{LINEAR_TOL, Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel::{self as kernel, Body, EdgeSpec, FaceSpec, SurfSpec};

use crate::sheet::FlatWork;
use crate::sheet_surface::Piece;
use crate::{DocError, Result};

/// How many times along a folded edge it is sampled, to tell a line or an arc.
const SAMPLES: usize = 16;
/// How far (mm) a folded edge's points may lie from the line or the arc it is taken for.
const ON_CURVE: f64 = 1e-9;
/// How far (mm) a chain of cubic pieces may lie from the folded edge it stands in for.
const CHAIN_TOL: f64 = 1e-7;
/// The most cubic pieces the kernel takes in one chain.
const MAX_CHAIN_PIECES: usize = 1024;
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
/// [`crate::sheet_surface::pieces`]), carried into space by `fold`.
pub(crate) fn solid(dev: &Development, fold: &Fold, t: f64, pieces: &[(Piece, Region2)], work: &mut FlatWork) -> Result<Body> {
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
        }
    }
    // Until the kernel sews faces that touch along an edge face to face (#1597): two walls on one
    // edge are refused as "ambiguous edge contact", so no edge may get a wall from both sides.
    Ok(kernel::sew_touching(&faces)?)
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
    /// line or an arc at both where it folds to one, else chains of cubic pieces split alike.
    fn edges(&self, piece: Piece, run: &[Seg2]) -> Result<[EdgeSpec; 2]> {
        let sampled = |depth: f64| (0..=SAMPLES).map(|k| self.at(piece, run_at(run, k as f64 / SAMPLES as f64), depth)).collect::<Result<Vec<_>>>();
        if let (Some(surface), Some(deep)) = (simple(&sampled(0.0)?), simple(&sampled(self.t)?)) {
            return Ok([surface, deep]);
        }
        // Halve any piece that strays at either depth; the run's own joints start pieces.
        let n = run.len().max(1);
        let mut breaks: Vec<f64> = (0..=n).map(|i| i as f64 / n as f64).collect();
        let mut k = 0;
        while let (Some(&s0), Some(&s1)) = (breaks.get(k), breaks.get(k + 1)) {
            let mut close = true;
            for depth in [0.0, self.t] {
                let c = self.cubic(piece, run, depth, s0, s1)?;
                for u in [0.25, 0.5, 0.75] {
                    close &= bezier(&c, u).dist(self.at(piece, run_at(run, s0 + (s1 - s0) * u), depth)?) <= CHAIN_TOL;
                }
            }
            if close {
                k += 1;
            } else if breaks.len() > MAX_CHAIN_PIECES {
                return Err(DocError::Invalid("an edge of the folded sheet bends too much to follow".into()));
            } else {
                breaks.insert(k + 1, (s0 + s1) / 2.0);
            }
        }
        let chain = |depth: f64| -> Result<EdgeSpec> {
            let mut ctrl = Vec::with_capacity(3 * breaks.len());
            for (k, w) in breaks.windows(2).enumerate() {
                let c = self.cubic(piece, run, depth, w[0], w[1])?;
                ctrl.extend_from_slice(if k == 0 { &c } else { &c[1..] });
            }
            Ok(EdgeSpec::Chain { degree: 3, ctrl: Arc::from(ctrl) })
        };
        Ok([chain(0.0)?, chain(self.t)?])
    }

    /// The cubic Hermite piece of a folded run from `s0` to `s1` along it: its ends and its
    /// direction there, taken from inside the piece, so that none is measured across a corner.
    fn cubic(&self, piece: Piece, run: &[Seg2], depth: f64, s0: f64, s1: f64) -> Result<[Vec3; 4]> {
        let at = |s: f64| self.at(piece, run_at(run, s), depth);
        let rate = |s: f64, h: f64| -> Result<Vec3> { Ok((at(s)? * -3.0 + at(s + h)? * 4.0 - at(s + 2.0 * h)?) * ((s1 - s0) / (2.0 * h))) };
        let (p0, p3, h) = (at(s0)?, at(s1)?, (s1 - s0) * STEP);
        Ok([p0, p0 + rate(s0, h)? * (1.0 / 3.0), p3 - rate(s1, -h)? * (1.0 / 3.0), p3])
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

fn bezier(c: &[Vec3; 4], u: f64) -> Vec3 {
    let w = 1.0 - u;
    c[0] * (w * w * w) + c[1] * (3.0 * w * w * u) + c[2] * (3.0 * w * u * u) + c[3] * (u * u * u)
}

/// The surface of the wall between an edge's folds on the surface (`bottom`) and a thickness in
/// (`top`), both running the same way.
fn wall(bottom: &EdgeSpec, top: &EdgeSpec) -> Result<SurfSpec> {
    match (bottom, top) {
        (EdgeSpec::Line { .. }, EdgeSpec::Line { .. }) => Ok(SurfSpec::Plane),
        (&EdgeSpec::Arc { a, b, mid }, &EdgeSpec::Arc { a: ta, b: tb, mid: tm }) => {
            let (c, n) = circle(a, mid, b).ok_or_else(|| DocError::Invalid("an arc of the folded sheet has no circle".into()))?;
            // In one plane: a bend's or a roll's end.
            if [ta, tb, tm].iter().all(|p| (*p - c).dot(n).abs() <= ON_CURVE) {
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
