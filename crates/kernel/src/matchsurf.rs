//! Surface matching: rebuild a surface body's face so that one boundary of it meets a picked
//! edge chain of another body up to a requested continuity (G1, G2 or G3), while the boundary
//! opposite it does not move.
//!
//! A match is the bridge core ([`crate::bridge`]) pointed at a different pair of boundaries. One
//! end reads its position, tangent, curvature and change of curvature from the *target's* face
//! along the picked chain — that is the end that moves. The other end reads them from the body's
//! own surface along the boundary opposite it, built through the original boundary's own points:
//! a match may shape the middle, never that seam. In between the cross-sections are the bridge's
//! cubic Hermites, whose crown is free, so the crown whose surface stays closest to the original
//! one — sampled on a grid and measured against it — is the one kept: a match moves the seam and
//! as little else as the conditions allow.
//!
//! Which boundary of the face moves is read off the geometry: the one nearest the picked chain.
//! The order a match may ask for is read off the target the same way: a chain that several faces
//! of the target meet, with shapes that part company, is a seam the target itself does not hold
//! beyond that order, and no surface can be told to carry a condition the target has not got.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{self as b, eval::CurveEval, eval::SurfaceEval};

use crate::body::Body;
use crate::bridge::{self, Continuity, Frame};
use crate::history::{Change, EdgeProv, History};
use crate::surfaces::Chain;
use crate::{KernelError, Result};

/// Stations a match reads along the seam and along the boundary it keeps.
const STATIONS: usize = 33;

/// Samples each way of the grid the rebuilt surface is compared with the original on.
const GRID: usize = 7;

/// The crowns the match tries at each end: [`crate::bridge::bridge`]'s free knob, which shapes
/// the middle without touching either end's conditions.
const CROWNS: [f64; 5] = [-0.5, 0.0, 0.5, 1.0, 2.0];

/// Points a caller may pick one face with.
const MAX_PICKS: usize = 64;

/// Samples per edge of a chain, for lengths and distances.
const PER_EDGE: usize = 32;

/// A seam is credited with G1 when its normals agree this closely (degrees), with G2 when the
/// two curvature vectors agree within this fraction of the larger of them, and with G3 when the
/// change of curvature agrees within this fraction of the curvature over the body's size.
const G1_DEG: f64 = 0.05;
const G2_REL: f64 = 5e-3;
const G3_REL: f64 = 5e-2;

/// Two faces of the target count as meeting each other at the picked chain when their normals
/// agree this closely, their curvature within this fraction of the larger, and their change of
/// curvature within this fraction of the curvature over the body's size.
const SUPPLY_DEG: f64 = 0.05;
const SUPPLY_REL: f64 = 2e-2;
const SUPPLY_BEND_REL: f64 = 1e-1;

/// Rebuild the face of `body` picked by `faces` (points on it) so that its boundary nearest the
/// chain of `target`'s edges picked by `target_pick` takes that chain's place and repeats the
/// target's own shape along it up to `order` — position, tangent, curvature vector, and the
/// change of that curvature vector along the direction the match leaves in. The boundary
/// opposite keeps its place and the original surface's own frame, and the middle is shaped to
/// stay as close to the original surface as the conditions allow. Its history is over
/// `[body, target]`: the face is the body's face, modified; its edges are new.
pub fn match_surface(body: &Body, faces: &[Vec3], target: &Body, target_pick: &[Vec3], order: Continuity) -> Result<Body> {
    body.require_brep("match")?;
    target.require_brep("match")?;
    if !body.is_surface() {
        return Err(KernelError::Invalid("match rebuilds a surface body: this one is closed".into()));
    }
    let (record, target_record) = (body.record(), target.record());
    let face = face_near(record, faces, body.size() * 0.05 + 1e-6)?;
    let original = Patch::of(record, face)?;
    let chain = bridge::chain(target, target_pick)?;
    let line = chain_polyline(target_record, &chain)?;
    let near = nearest_side(&original, &line)?;
    let far = near.opposite();
    let far_edges = side_edges(record, face, &original, far, body.size() * 1e-6 + 1e-9)?;
    let (mid_near, mid_far) = (chain_mid(&line)?, original.at(far, 0.5)?);
    let leave = (mid_far - mid_near)
        .normalized()
        .filter(|l| l.is_finite())
        .ok_or_else(|| KernelError::Invalid("the far boundary sits on the picked chain: there is nothing to match".into()))?;
    // Which face of the target the match continues from, and how much of the requested order the
    // target itself holds along that chain.
    let candidates = support_faces(target, &chain, &line, leave)?;
    let supply = supply_order(target, &candidates, &chain, leave)?;
    if order.conditions() > supply.conditions() {
        return Err(KernelError::Invalid(format!(
            "the target's own surfaces part company at the picked edges: they hold only {} there, so a seam cannot hold {}",
            supply.as_str(),
            order.as_str()
        )));
    }
    let Some(&(target_face, _)) = candidates.first() else {
        return Err(KernelError::Invalid("no face of the target holds the picked edges".into()));
    };
    // The stations of the seam: what the target does there along the direction the match leaves
    // in, and what the far boundary does coming back the other way.
    let mut near_frames = bridge::frames(target, target_face, &chain, leave, STATIONS)?;
    let far_frames = bridge::frames(body, face, &far_edges, leave * -1.0, STATIONS)?;
    // The two chains are sampled from their own starts: turn one round when that pairs the
    // stations better (a match is built station to station).
    let direct: f64 = near_frames.iter().zip(&far_frames).map(|(a, b)| a.p.dist(b.p)).sum();
    let reversed: f64 = near_frames.iter().zip(far_frames.iter().rev()).map(|(a, b)| a.p.dist(b.p)).sum();
    if reversed < direct {
        near_frames.reverse();
    }
    // The crown is the free knob: keep the one whose surface stays closest to the original.
    let mut best: Option<(f64, Body)> = None;
    for crown_near in CROWNS {
        for crown_far in CROWNS {
            let made = bridge::bridge(&near_frames, &far_frames, [order, Continuity::G1], [crown_near, crown_far])?;
            let stray = deviation(&Patch::of(made.record(), 0)?, &original)?;
            if best.as_ref().is_none_or(|(b, _)| stray < *b) {
                best = Some((stray, made));
            }
        }
    }
    let Some((_, made)) = best else {
        return Err(KernelError::Failed("match: no crown built a surface".into()));
    };
    // The rebuilt face takes the target's own sense at the seam: the two surfaces then meet the
    // same way round, which is what a stitch, a thicken or a display of the result reads.
    let seam = chain_mid(&line)?;
    let made_record = made.record();
    let turn = match (Patch::of(made_record, 0)?.normal_near(seam), Patch::of(target_record, target_face)?.normal_near(seam)) {
        (Some((m, _)), Some((t, _))) => m.dot(t) < 0.0,
        _ => false,
    };
    let kept = crate::surfaces::Parts::of(made_record, &[(0, turn)])?.canonical()?;
    let sheet = crate::surfaces::surface_body(&kept.brep)?;
    let history = History {
        operands: 2,
        faces: vec![crate::record::from_face(0, face, Change::Modified)],
        edges: (0..sheet.edge_count()).map(|_| EdgeProv { faces: vec![0], origins: Vec::new() }).collect(),
    };
    sheet.with_history(history, &[body, target])
}

/// How well the rebuilt surface `made` meets `target` along the chain picked by `target_pick`:
/// the worst distance from the seam to that chain, the worst angle between the two surfaces'
/// normals across it (degrees), and the highest continuity the seam holds
/// ([`Continuity::G0`] when even the normals disagree).
pub fn seam_continuity(made: &Body, target: &Body, target_pick: &[Vec3]) -> Result<(f64, f64, Continuity)> {
    made.require_brep("match")?;
    target.require_brep("match")?;
    let (record, target_record) = (made.record(), target.record());
    let chain = bridge::chain(target, target_pick)?;
    let poly = chain_polyline(target_record, &chain)?;
    // The face whose boundary runs along the chain: the one nearest it, so that a body that has
    // not been matched yet — where the seam is still some way off — can be measured too.
    let mid = chain_mid(&poly)?;
    let face = (0..record.faces.len())
        .filter_map(|f| Patch::of(record, f).ok().and_then(|p| p.normal_near(mid).map(|(_, d)| (d, p))))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p)
        .ok_or_else(|| KernelError::Failed("match: the body has no faces".into()))?;
    let near = nearest_side(&face, &poly)?;
    let far = near.opposite();
    let leave = (face.at(far, 0.5)? - mid)
        .normalized()
        .ok_or_else(|| KernelError::Invalid("the seam and the far boundary coincide: there is nothing to measure".into()))?;
    let candidates = support_faces(target, &chain, &poly, leave)?;
    let Some(&(target_face, _)) = candidates.first() else {
        return Err(KernelError::Invalid("no face of the target holds the picked edges".into()));
    };
    let target_patch = Patch::of(target_record, target_face)?;
    let frames = bridge::frames(target, target_face, &chain, leave, STATIONS)?;
    let size = target.size();
    let (mut gap, mut deg, mut curv, mut bend) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for i in 0..STATIONS {
        let t = i as f64 / (STATIONS - 1) as f64;
        let p = face.at(near, t)?;
        gap = gap.max(chain_distance(&poly, p));
        // What the target says there, and what the rebuilt surface actually does there.
        let Some(frame) = frames.iter().min_by(|a, b| a.p.dist(p).total_cmp(&b.p.dist(p))) else {
            return Err(KernelError::Failed("match: the target has no frames".into()));
        };
        let (_, t_made, bend_made, change_made) = face.seam_shape(near, t)?;
        if let (Some((n_made, _)), Some((n_target, _))) = (face.normal_near(p), target_patch.normal_near(p)) {
            deg = deg.max(angle_deg(n_made, n_target));
        }
        let scale = frame.bend.len().max(bend_made.len()).max(1.0 / size);
        curv = curv.max((bend_made - frame.bend).len() / scale);
        // The change of curvature is what a surface can hold: only its normal part counts, and it
        // turns round with the direction of travel.
        let change = if t_made.dot(frame.leave) < 0.0 { change_made * -1.0 } else { change_made };
        let scale3 = frame.bend_change.len().max(change.len()).max(scale / size);
        bend = bend.max((change - frame.bend_change).len() / scale3);
    }
    let mut order = Continuity::G0;
    if deg <= G1_DEG {
        order = Continuity::G1;
        if curv <= G2_REL {
            order = Continuity::G2;
            if bend <= G3_REL {
                order = Continuity::G3;
            }
        }
    }
    Ok((gap, deg, order))
}

/// Which side of a face's parameter rectangle a boundary runs along: the match reads its face as
/// a tensor patch, so its four sides are its own parameter lines.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    /// The `u = u0` side (its points run along `v`).
    U0,
    /// The `u = u1` side.
    U1,
    /// The `v = v0` side (its points run along `u`).
    V0,
    /// The `v = v1` side.
    V1,
}

impl Side {
    /// The four sides, in a fixed order.
    const ALL: [Side; 4] = [Side::U0, Side::U1, Side::V0, Side::V1];

    /// The side opposite this one: what a match keeps when this one moves.
    fn opposite(self) -> Side {
        match self {
            Side::U0 => Side::U1,
            Side::U1 => Side::U0,
            Side::V0 => Side::V1,
            Side::V1 => Side::V0,
        }
    }

    /// Whether this side pins `u` (its points run along `v`).
    fn is_u(self) -> bool {
        matches!(self, Side::U0 | Side::U1)
    }

    /// The value this side pins, and the range the side runs over.
    fn fixed_and_along(self, u: (f64, f64), v: (f64, f64)) -> (f64, (f64, f64)) {
        match self {
            Side::U0 => (u.0, v),
            Side::U1 => (u.1, v),
            Side::V0 => (v.0, u),
            Side::V1 => (v.1, u),
        }
    }

    /// The point at the normalized station `t` along the side.
    fn at(surf: &SurfaceEval, ranges: &[(f64, f64); 2], side: Side, t: f64) -> Result<Vec3> {
        let (u, v) = (ranges[0], ranges[1]);
        let (fixed, along) = side.fixed_and_along(u, v);
        let t = along.0 + (along.1 - along.0) * t.clamp(0.0, 1.0);
        surf.point(if side.is_u() { [fixed, t] } else { [t, fixed] }).ok_or_else(|| KernelError::Failed("match: a side has no point there".into()))
    }
}

/// A face read as a tensor patch: its record surface and its parameter box.
struct Patch {
    surface: SurfaceEval,
    ranges: [(f64, f64); 2],
}

impl Patch {
    fn of(r: &b::Brep, face: usize) -> Result<Patch> {
        let f = r.faces.get(face).ok_or_else(|| KernelError::Invalid(format!("no face {face}")))?;
        let surface = SurfaceEval::new(f.parameters()).map_err(|e| KernelError::Failed(format!("match: face {face}: {e}")))?;
        let [[u0, u1], [v0, v1]] = f.domain;
        Ok(Patch { surface, ranges: [(u0, u1), (v0, v1)] })
    }

    fn at(&self, side: Side, t: f64) -> Result<Vec3> {
        Side::at(&self.surface, &self.ranges, side, t)
    }

    /// The surface's unit normal at the point nearest `p`, and how far `p` is from that point.
    fn normal_near(&self, p: Vec3) -> Option<(Vec3, f64)> {
        let uv = self.surface.project(p, None, f64::INFINITY)?;
        let (q, su, sv) = self.surface.derivs(uv[0], uv[1])?;
        Some((su.cross(sv).normalized()?, q.dist(p)))
    }

    /// The parameters of the point nearest `p`.
    fn parameters(&self, p: Vec3) -> Option<[f64; 2]> {
        self.surface.project(p, None, f64::INFINITY)
    }

    /// The shape of the surface along its cross direction at the station `t`, read inwards from
    /// the boundary `near` over five one-sided samples: the point, the unit tangent, the
    /// curvature vector and the change of that curvature. The same finite difference
    /// [`crate::bridge::frames`] reads a support surface with, so that a rebuilt seam can be
    /// compared with what it matched.
    fn seam_shape(&self, near: Side, t: f64) -> Result<(Vec3, Vec3, Vec3, Vec3)> {
        let ranges = self.ranges;
        let (station, cross, at_start) = if near.is_u() {
            (ranges[1].0 + (ranges[1].1 - ranges[1].0) * t, ranges[0], near == Side::U0)
        } else {
            (ranges[0].0 + (ranges[0].1 - ranges[0].0) * t, ranges[1], near == Side::V0)
        };
        let span = (cross.1 - cross.0).abs().max(1e-12);
        let h = span * 1e-3;
        let c0 = if at_start { cross.0 } else { cross.1 };
        let sgn = if at_start { 1.0 } else { -1.0 };
        let at = |x: f64| self.surface.point(if near.is_u() { [x, station] } else { [station, x] });
        let q: Vec<Vec3> =
            (0..5).map(|k| at(c0 + sgn * k as f64 * h)).collect::<Option<_>>().ok_or_else(|| KernelError::Failed("match: seam samples".into()))?;
        let (Some(&q0), Some(&q1), Some(&q2), Some(&q3), Some(&q4)) = (q.first(), q.get(1), q.get(2), q.get(3), q.get(4)) else {
            return Err(KernelError::Failed("match: seam samples".into()));
        };
        let d1 = (q0 * -25.0 + q1 * 48.0 + q2 * -36.0 + q3 * 16.0 + q4 * -3.0) * (sgn / (12.0 * h));
        let d2 = (q0 * 35.0 + q1 * -104.0 + q2 * 114.0 + q3 * -56.0 + q4 * 11.0) * (1.0 / (12.0 * h * h));
        let d3 = (q0 * -5.0 + q1 * 18.0 + q2 * -24.0 + q3 * 14.0 + q4 * -3.0) * (sgn / (2.0 * h * h * h));
        let (t, bend, change) = shape(d1, d2, d3)?;
        Ok((q0, t, bend, change))
    }
}

/// The face of the record the picks lie on: they must all pick the same one, each within `reach`
/// of its surface (a pick is a point the caller clicked).
fn face_near(r: &b::Brep, picks: &[Vec3], reach: f64) -> Result<usize> {
    if picks.is_empty() || picks.len() > MAX_PICKS || picks.iter().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid(format!("pick 1 to {MAX_PICKS} points on the face to match")));
    }
    let mut found: Option<usize> = None;
    for p in picks {
        let hit = (0..r.faces.len())
            .filter_map(|f| Patch::of(r, f).ok().and_then(|x| x.normal_near(*p)).map(|(_, d)| (d, f)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .filter(|(d, _)| *d <= reach)
            .map(|(_, f)| f)
            .ok_or_else(|| KernelError::Invalid(format!("no face of the body passes through {p:?}")))?;
        if found.is_some_and(|f| f != hit) {
            return Err(KernelError::Invalid("the picks are on different faces: match one face".into()));
        }
        found = Some(hit);
    }
    found.ok_or_else(|| KernelError::Invalid("pick a point on the face to match".into()))
}

/// A chain of the record's edges as a polyline (each edge sampled the chain's way), for lengths
/// and distances.
fn chain_polyline(r: &b::Brep, chain: &Chain) -> Result<Vec<Vec3>> {
    if chain.is_empty() {
        return Err(KernelError::Invalid("the chain has no edges".into()));
    }
    let mut out = Vec::with_capacity(chain.len() * (PER_EDGE + 1));
    for &(e, forward) in chain {
        let edge = r.edges.get(e).ok_or_else(|| KernelError::Invalid("an edge reference".into()))?;
        let c = CurveEval::new(&edge.curve).map_err(|err| KernelError::Failed(format!("match: {err}")))?;
        let [t0, t1] = edge.interval;
        let (a, z) = if forward { (t0, t1) } else { (t1, t0) };
        for k in 0..=PER_EDGE {
            out.push(c.point(a + (z - a) * k as f64 / PER_EDGE as f64).ok_or_else(|| KernelError::Failed("match: an edge has no point".into()))?);
        }
    }
    Ok(out)
}

/// The point at the middle of a polyline, by length.
fn chain_mid(poly: &[Vec3]) -> Result<Vec3> {
    let mut cum = Vec::with_capacity(poly.len());
    cum.push(0.0);
    for w in poly.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let Some(last) = cum.last().copied() else { break };
        cum.push(last + a.dist(*b));
    }
    let total = cum.last().copied().unwrap_or(0.0);
    if !(total > 1e-9) || !total.is_finite() {
        return Err(KernelError::Invalid("the picked chain has no length".into()));
    }
    let half = total * 0.5;
    let hi = cum.partition_point(|x| *x < half).clamp(1, poly.len().saturating_sub(1));
    let (Some(a), Some(b), Some(&lo), Some(&up)) = (poly.get(hi - 1), poly.get(hi), cum.get(hi - 1), cum.get(hi)) else {
        return Err(KernelError::Failed("match: chain".into()));
    };
    let span = up - lo;
    let t = if span > 1e-12 { (half - lo) / span } else { 0.0 };
    Ok(*a + (*b - *a) * t)
}

/// How far `p` is from a polyline.
fn chain_distance(poly: &[Vec3], p: Vec3) -> f64 {
    let mut best = f64::INFINITY;
    for w in poly.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let d = b.dist(*a);
        let t = if d > 1e-12 { ((p - *a).dot(*b - *a) / (d * d)).clamp(0.0, 1.0) } else { 0.0 };
        best = best.min(p.dist(*a + (*b - *a) * t));
    }
    best
}

/// The side of the face's own parameter rectangle the picked chain (as a polyline) sits nearest.
fn nearest_side(face: &Patch, poly: &[Vec3]) -> Result<Side> {
    /// Samples of each side used to tell which one the chain runs along.
    const SAMPLES: usize = 17;
    let mut best: Option<(f64, Side)> = None;
    for side in Side::ALL {
        let mut total = 0.0;
        for k in 0..SAMPLES {
            total += chain_distance(poly, face.at(side, k as f64 / (SAMPLES - 1) as f64)?);
        }
        let mean = total / SAMPLES as f64;
        if best.is_none_or(|(b, _)| mean < b) {
            best = Some((mean, side));
        }
    }
    best.map(|(_, s)| s).ok_or_else(|| KernelError::Failed("match: the face has no sides".into()))
}

/// The face's own edges that run along a side of its parameter rectangle, chained head to tail.
/// A match keeps the far side's own edges, so they must be a chain that runs on.
fn side_edges(r: &b::Brep, face: usize, patch: &Patch, side: Side, tol: f64) -> Result<Chain> {
    let (fixed, along) = side.fixed_and_along(patch.ranges[0], patch.ranges[1]);
    let band = (along.1 - along.0).abs().max(1e-12) * 1e-3;
    let f = r.faces.get(face).ok_or_else(|| KernelError::Invalid(format!("no face {face}")))?;
    let mut pool: Vec<usize> = Vec::new();
    for l in &f.loops {
        let b::Loop::Edges { edges } = l else { continue };
        for u in edges {
            let e = u.edge as usize;
            let Some(edge) = r.edges.get(e) else { continue };
            let Ok(c) = CurveEval::new(&edge.curve) else { continue };
            let Some(p) = c.point(0.5 * (edge.interval[0] + edge.interval[1])) else { continue };
            let Some(uv) = patch.parameters(p) else { continue };
            if patch.surface.point(uv).is_none_or(|q| q.dist(p) > tol) {
                continue;
            }
            let got = if side.is_u() { uv[0] } else { uv[1] };
            if (got - fixed).abs() <= band && !pool.contains(&e) {
                pool.push(e);
            }
        }
    }
    if pool.is_empty() {
        return Err(KernelError::Invalid("the face has no edges along the boundary a match would keep".into()));
    }
    let ends = |e: usize| r.edges.get(e).map(|x| x.vertices).unwrap_or([u32::MAX; 2]);
    let first = pool.remove(0);
    let mut chain: Chain = vec![(first, true)];
    let [mut start, mut end] = ends(first);
    while let Some(k) = pool.iter().position(|e| ends(*e).contains(&end)) {
        let e = pool.remove(k);
        let [a, z] = ends(e);
        let forward = a == end;
        end = if forward { z } else { a };
        chain.push((e, forward));
    }
    while let Some(k) = pool.iter().position(|e| ends(*e).contains(&start)) {
        let e = pool.remove(k);
        let [a, z] = ends(e);
        let forward = z == start;
        start = if forward { a } else { z };
        chain.insert(0, (e, forward));
    }
    if !pool.is_empty() {
        return Err(KernelError::Invalid("the face's edges along that boundary do not run on from one another".into()));
    }
    Ok(chain)
}

/// The faces of `target` that bound every edge of the chain and whose tangent plane holds the
/// leave direction, best first: the face a match continues from, and the ones it might have been
/// read from.
fn support_faces(target: &Body, chain: &Chain, poly: &[Vec3], leave: Vec3) -> Result<Vec<(usize, f64)>> {
    let r = target.record();
    let mid = chain_mid(poly)?;
    let tol = target.size() * 1e-6 + 1e-9;
    let bounds = |f: usize, e: usize| r.edges.get(e).is_some_and(|x| x.uses.iter().any(|u| u.face as usize == f));
    let mut out: Vec<(usize, f64)> = Vec::new();
    for f in 0..r.faces.len() {
        if !chain.iter().all(|(e, _)| bounds(f, *e)) {
            continue;
        }
        let Some((n, d)) = Patch::of(r, f).ok().and_then(|p| p.normal_near(mid)) else { continue };
        if d > tol {
            continue;
        }
        let score = (leave - n * leave.dot(n)).len();
        if score.is_finite() && score >= 0.2 {
            out.push((f, score));
        }
    }
    if out.is_empty() {
        return Err(KernelError::Invalid(
            "no face of the target holds the picked edges in the direction the match leaves in: pick the chain of a face the surface can continue"
                .into(),
        ));
    }
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ok(out)
}

/// The highest order the target itself holds along the chain: every face that meets those edges
/// and whose tangent plane holds the leave direction reads the chain's shape from its own
/// surface, and where two of them part company the target's own geometry jumps at the seam.
fn supply_order(target: &Body, candidates: &[(usize, f64)], chain: &Chain, leave: Vec3) -> Result<Continuity> {
    let Some(&(first, best)) = candidates.first() else {
        return Err(KernelError::Invalid("no face of the target holds the picked edges".into()));
    };
    let keep: Vec<usize> = candidates.iter().filter(|(_, s)| *s >= 0.5 * best).map(|(f, _)| *f).collect();
    let mut order = Continuity::G3;
    if keep.len() < 2 {
        return Ok(order);
    }
    let r = target.record();
    let size = target.size();
    let first_patch = Patch::of(r, first)?;
    let primary: Vec<Frame> = bridge::frames(target, first, chain, leave, STATIONS)?;
    for &other in keep.iter().skip(1) {
        // A face that cannot be read along the whole chain says nothing here: the face the match
        // is built on is the one that must be able to supply it.
        let Ok(other_frames) = bridge::frames(target, other, chain, leave, STATIONS) else { continue };
        let other_patch = Patch::of(r, other)?;
        let mut worst_deg = 0.0f64;
        let mut worst = 0.0f64;
        let mut worst_change = 0.0f64;
        for (a, b) in primary.iter().zip(&other_frames) {
            if let (Some((na, _)), Some((nb, _))) = (first_patch.normal_near(a.p), other_patch.normal_near(b.p)) {
                worst_deg = worst_deg.max(angle_deg(na, nb));
            }
            let scale = a.bend.len().max(b.bend.len()).max(1.0 / size);
            worst = worst.max((a.bend - b.bend).len() / scale);
            let scale3 = a.bend_change.len().max(b.bend_change.len()).max(scale / size);
            worst_change = worst_change.max((a.bend_change - b.bend_change).len() / scale3);
        }
        if worst_deg > SUPPLY_DEG {
            order = Continuity::G0;
        }
        if worst > SUPPLY_REL && order.conditions() > 1 {
            order = Continuity::G1;
        }
        if worst_change > SUPPLY_BEND_REL && order.conditions() > 2 {
            order = Continuity::G2;
        }
    }
    Ok(order)
}

/// The acute angle between two directions, in degrees. For two surfaces' normals this is the
/// angle between their tangent planes: a sheet has no inside, so which way a normal points says
/// nothing about whether the surfaces meet smoothly.
fn angle_deg(a: Vec3, b: Vec3) -> f64 {
    let d = a.dot(b) / (a.len() * b.len());
    if !d.is_finite() {
        return f64::NAN;
    }
    d.abs().clamp(0.0, 1.0).acos().to_degrees()
}

/// The geometric shape of a surface curve at a point from its first three derivatives along the
/// curve: the same statement [`crate::bridge`] makes of a support surface — the unit tangent, the
/// curvature vector, and the arclength derivative of that curvature vector.
fn shape(d1: Vec3, d2: Vec3, d3: Vec3) -> Result<(Vec3, Vec3, Vec3)> {
    let sigma = d1.len();
    if !(sigma > 1e-12) || !sigma.is_finite() {
        return Err(KernelError::Failed("match: the seam's cross-section does not move".into()));
    }
    let t = d1 * (1.0 / sigma);
    let tp = d2 * (1.0 / sigma) - d1 * (d1.dot(d2) / (sigma * sigma * sigma));
    let nn = d2 - t * d2.dot(t);
    let np = d3 - t * d3.dot(t) - tp * d2.dot(t) - t * d2.dot(tp);
    let kappa = np * (sigma * sigma) - nn * (2.0 * d1.dot(d2));
    let change = kappa * (1.0 / (sigma.powi(5)));
    if ![t, nn, change].into_iter().all(Vec3::is_finite) {
        return Err(KernelError::Failed("match: the seam's shape is not finite".into()));
    }
    Ok((t, nn * (1.0 / (sigma * sigma)), change))
}

/// How far the rebuilt surface strays from the original one: the root mean square of the
/// distances from a grid of its samples to the original surface.
fn deviation(made: &Patch, original: &Patch) -> Result<f64> {
    let ranges = made.ranges;
    let mut sum = 0.0f64;
    let mut n = 0usize;
    for i in 0..GRID {
        for j in 0..GRID {
            let u = ranges[0].0 + (ranges[0].1 - ranges[0].0) * i as f64 / (GRID - 1) as f64;
            let v = ranges[1].0 + (ranges[1].1 - ranges[1].0) * j as f64 / (GRID - 1) as f64;
            let Some(q) = made.surface.point([u, v]) else { continue };
            let Some((_, d)) = original.normal_near(q) else { continue };
            if d.is_finite() {
                sum += d * d;
                n += 1;
            }
        }
    }
    if n == 0 {
        return Err(KernelError::Failed("match: the original surface cannot be sampled".into()));
    }
    Ok((sum / n as f64).sqrt())
}

#[cfg(test)]
#[path = "matchsurf_tests.rs"]
mod tests;
