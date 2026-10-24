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

use mt::{BoundedCurve, ParametricCurve, ParametricSurface, ParametricSurface3D, SearchNearestParameter};
use solidcraft_geom::{MinByFinite, Vec3};
use truck_modeling as mt;

use crate::body::{Body, from_p3, p3};
use crate::bridge::{self, Continuity, Frame};
use crate::record::Rebuild;
use crate::surfaces::surface_body;
use crate::{KernelError, Result, guard};

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
/// stay as close to the original surface as the conditions allow.
pub fn match_surface(body: &Body, faces: &[Vec3], target: &Body, target_pick: &[Vec3], order: Continuity) -> Result<Body> {
    body.require_brep("match")?;
    target.require_brep("match")?;
    if !body.is_surface() {
        return Err(KernelError::Invalid("match rebuilds a surface body: this one is closed".into()));
    }
    let face = face_near(body, faces)?;
    let chain = bridge::chain(target, target_pick)?;
    let near = nearest_side(&face, &chain)?;
    let far = near.opposite();
    let far_edges = side_edges(&face, far, body.size() * 1e-6 + 1e-9)?;
    let (mid_near, mid_far) = (chain_mid(&chain)?, Side::at(&face.surface(), &ranges(&face)?, far, 0.5));
    let leave = (mid_far - mid_near)
        .normalized()
        .filter(|l| l.is_finite())
        .ok_or_else(|| KernelError::Invalid("the far boundary sits on the picked chain: there is nothing to match".into()))?;
    // Which face of the target the match continues from, and how much of the requested order the
    // target itself holds along that chain.
    let candidates = support_faces(target, &chain, leave)?;
    let supply = supply_order(&candidates, &chain, leave, target.size())?;
    if order.conditions() > supply.conditions() {
        return Err(KernelError::Invalid(format!(
            "the target's own surfaces part company at the picked edges: they hold only {} there, so a seam cannot hold {}",
            supply.as_str(),
            order.as_str()
        )));
    }
    let Some((target_face, _)) = candidates.first() else {
        return Err(KernelError::Invalid("no face of the target holds the picked edges".into()));
    };
    // The stations of the seam: what the target does there along the direction the match leaves
    // in, and what the far boundary does coming back the other way.
    let mut near_frames = bridge::frames(target_face, &chain, leave, STATIONS)?;
    let far_frames = bridge::frames(&face, &far_edges, leave * -1.0, STATIONS)?;
    // The two chains are sampled from their own starts: turn one round when that pairs the
    // stations better (a match is built station to station).
    let direct: f64 = near_frames.iter().zip(&far_frames).map(|(a, b)| a.p.dist(b.p)).sum();
    let reversed: f64 = near_frames.iter().zip(far_frames.iter().rev()).map(|(a, b)| a.p.dist(b.p)).sum();
    if reversed < direct {
        near_frames.reverse();
    }
    guard("surface match", || {
        // The crown is the free knob: keep the one whose surface stays closest to the original.
        let mut best: Option<(f64, mt::Face)> = None;
        for crown_near in CROWNS {
            for crown_far in CROWNS {
                let made = bridge::bridge(&near_frames, &far_frames, [order, Continuity::G1], [crown_near, crown_far])?;
                let Some(made) = made.solid.face_iter().next().cloned() else {
                    return Err(KernelError::Failed("match: the bridge made no face".into()));
                };
                let stray = deviation(&made, &face)?;
                if best.as_ref().is_none_or(|(b, _)| stray < *b) {
                    best = Some((stray, made));
                }
            }
        }
        let Some((_, made)) = best else {
            return Err(KernelError::Failed("match: no crown built a surface".into()));
        };
        let mut kept = made;
        // The rebuilt face takes the target's own sense at the seam: the two surfaces then meet
        // the same way round, which is what a stitch, a thicken or a display of the result reads.
        let seam = chain_mid(&chain)?;
        if let (Some((made_n, _)), Some((target_n, _))) = (normal_near(&kept, seam), normal_near(target_face, seam))
            && made_n.dot(target_n) < 0.0
        {
            kept.invert();
        }
        let mut rec = Rebuild::default();
        rec.face(&kept, &face);
        Ok(crate::record::rebuilt_body(surface_body(vec![kept]), &[&body.solid, &target.solid], &rec))
    })
}

/// How well the rebuilt surface `made` meets `target` along the chain picked by `target_pick`:
/// the worst distance from the seam to that chain, the worst angle between the two surfaces'
/// normals across it (degrees), and the highest continuity the seam holds
/// ([`Continuity::G0`] when even the normals disagree).
pub fn seam_continuity(made: &Body, target: &Body, target_pick: &[Vec3]) -> Result<(f64, f64, Continuity)> {
    made.require_brep("match")?;
    target.require_brep("match")?;
    let chain = bridge::chain(target, target_pick)?;
    let poly = chain_polyline(&chain)?;
    // The face whose boundary runs along the chain: the one nearest it, so that a body that has
    // not been matched yet — where the seam is still some way off — can be measured too.
    let mid = chain_mid(&chain)?;
    let face = made
        .solid
        .face_iter()
        .filter_map(|f| face_distance(f, mid).map(|d| (d, f)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, f)| f.clone())
        .ok_or_else(|| KernelError::Failed("match: the body has no faces".into()))?;
    let near = nearest_side(&face, &chain)?;
    let far = near.opposite();
    let ranges = ranges(&face)?;
    let mid_far = Side::at(&face.surface(), &ranges, far, 0.5);
    let leave = (mid_far - chain_mid(&chain)?)
        .normalized()
        .ok_or_else(|| KernelError::Invalid("the seam and the far boundary coincide: there is nothing to measure".into()))?;
    let candidates = support_faces(target, &chain, leave)?;
    let Some((target_face, _)) = candidates.first() else {
        return Err(KernelError::Invalid("no face of the target holds the picked edges".into()));
    };
    let frames = bridge::frames(target_face, &chain, leave, STATIONS)?;
    let size = target.size();
    guard("surface match", || {
        let surf = face.surface();
        let (mut gap, mut deg, mut curv, mut bend) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for i in 0..STATIONS {
            let t = i as f64 / (STATIONS - 1) as f64;
            let p = Side::at(&surf, &ranges, near, t);
            gap = gap.max(chain_distance(&poly, p));
            // What the target says there, and what the rebuilt surface actually does there.
            let Some(frame) = frames.iter().min_by(|a, b| a.p.dist(p).total_cmp(&b.p.dist(p))) else {
                return Err(KernelError::Failed("match: the target has no frames".into()));
            };
            let (_, t_made, bend_made, change_made) = seam_shape(&surf, &ranges, near, t)?;
            if let Some((n_made, _)) = normal_near(&face, p)
                && let Some((n_target, _)) = normal_near(target_face, p)
            {
                deg = deg.max(angle_deg(n_made, n_target));
            }
            let scale = frame.bend.len().max(bend_made.len()).max(1.0 / size);
            curv = curv.max((bend_made - frame.bend).len() / scale);
            // The change of curvature is what a surface can hold: only its normal part counts,
            // and it turns round with the direction of travel.
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
    })
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
    fn at(surf: &mt::Surface, ranges: &[(f64, f64); 2], side: Side, t: f64) -> Vec3 {
        let (u, v) = (ranges[0], ranges[1]);
        let (fixed, along) = side.fixed_and_along(u, v);
        let t = along.0 + (along.1 - along.0) * t.clamp(0.0, 1.0);
        if side.is_u() { from_p3(surf.subs(fixed, t)) } else { from_p3(surf.subs(t, fixed)) }
    }
}

/// The bounded parameter ranges of a face's surface.
fn ranges(face: &mt::Face) -> Result<[(f64, f64); 2]> {
    let surf = face.surface();
    let (Some(u), Some(v)) = surf.try_range_tuple() else {
        return Err(KernelError::Failed("match: the face's surface is unbounded".into()));
    };
    Ok([u, v])
}

/// How far `p` is from the face's own surface.
fn face_distance(face: &mt::Face, p: Vec3) -> Option<f64> {
    let surf = face.surface();
    let (u, v) = surf.search_nearest_parameter(p3(p), None, 100)?;
    Some(from_p3(surf.subs(u, v)).dist(p))
}

/// The surface's unit normal at the point nearest `p`, and how far `p` is from that point.
fn normal_near(face: &mt::Face, p: Vec3) -> Option<(Vec3, f64)> {
    let surf = face.surface();
    let (u, v) = surf.search_nearest_parameter(p3(p), None, 100)?;
    let n = surf.normal(u, v);
    let n = Vec3::new(n.x, n.y, n.z).normalized()?;
    Some((n, from_p3(surf.subs(u, v)).dist(p)))
}

/// The face of `body` the picks lie on: they must all pick the same one. A pick is a point the
/// caller clicked, so the face nearest it within the body's own pick tolerance is the face it
/// means (`crate::surfaces::edge_near` reads points the same way).
fn face_near(body: &Body, picks: &[Vec3]) -> Result<mt::Face> {
    if picks.is_empty() || picks.len() > MAX_PICKS || picks.iter().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid(format!("pick 1 to {MAX_PICKS} points on the face to match")));
    }
    let tol = body.size() * 0.05 + 1e-6;
    let mut found: Option<mt::Face> = None;
    for p in picks {
        let hit = body
            .solid
            .face_iter()
            .filter_map(|f| face_distance(f, *p).map(|d| (d, f)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .filter(|(d, _)| *d <= tol)
            .map(|(_, f)| f.clone())
            .ok_or_else(|| KernelError::Invalid(format!("no face of the body passes through {p:?}")))?;
        if found.as_ref().is_some_and(|f| f.id() != hit.id()) {
            return Err(KernelError::Invalid("the picks are on different faces: match one face".into()));
        }
        found = Some(hit);
    }
    found.ok_or_else(|| KernelError::Invalid("pick a point on the face to match".into()))
}

/// A chain of edges as a polyline (each edge sampled), for lengths and distances.
fn chain_polyline(chain: &[mt::Edge]) -> Result<Vec<Vec3>> {
    if chain.is_empty() {
        return Err(KernelError::Invalid("the chain has no edges".into()));
    }
    let mut out = Vec::with_capacity(chain.len() * (PER_EDGE + 1));
    for e in chain {
        let c = e.curve();
        let (t0, t1) = c.range_tuple();
        if !(t1 > t0) || ![t0, t1].into_iter().all(f64::is_finite) {
            return Err(KernelError::Failed("match: an edge has no parameter range".into()));
        }
        for k in 0..=PER_EDGE {
            out.push(from_p3(c.subs(t0 + (t1 - t0) * k as f64 / PER_EDGE as f64)));
        }
    }
    Ok(out)
}

/// The point at the middle of a chain, by length.
fn chain_mid(chain: &[mt::Edge]) -> Result<Vec3> {
    let poly = chain_polyline(chain)?;
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
    let (Some(a), Some(b)) = (poly.get(hi - 1), poly.get(hi)) else {
        return Err(KernelError::Failed("match: chain".into()));
    };
    let (Some(&lo), Some(&up)) = (cum.get(hi - 1), cum.get(hi)) else {
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

/// The side of the face's own parameter rectangle the picked chain sits nearest.
fn nearest_side(face: &mt::Face, chain: &[mt::Edge]) -> Result<Side> {
    let surf = face.surface();
    let ranges = ranges(face)?;
    let poly = chain_polyline(chain)?;
    /// Samples of each side used to tell which one the chain runs along.
    const SAMPLES: usize = 17;
    let mean = |side: Side| {
        let total: f64 = (0..SAMPLES).map(|k| chain_distance(&poly, Side::at(&surf, &ranges, side, k as f64 / (SAMPLES - 1) as f64))).sum();
        total / SAMPLES as f64
    };
    Side::ALL.into_iter().min_by_finite(|side| mean(*side)).ok_or_else(|| KernelError::Failed("match: the face has no sides".into()))
}

/// The face's own edges that run along a side of its parameter rectangle, chained head to tail.
/// A match keeps the far side's own edges, so they must be a chain that runs on.
fn side_edges(face: &mt::Face, side: Side, tol: f64) -> Result<Vec<mt::Edge>> {
    let surf = face.surface();
    let ranges = ranges(face)?;
    let (fixed, along) = side.fixed_and_along(ranges[0], ranges[1]);
    let band = (along.1 - along.0).abs().max(1e-12) * 1e-3;
    let mut pool: Vec<mt::Edge> = Vec::new();
    for e in face.edge_iter() {
        let c = e.curve();
        let (t0, t1) = c.range_tuple();
        let p = from_p3(c.subs(0.5 * (t0 + t1)));
        let Some((u, v)) = surf.search_nearest_parameter(p3(p), None, 100) else { continue };
        if from_p3(surf.subs(u, v)).dist(p) > tol {
            continue;
        }
        let got = if side.is_u() { u } else { v };
        if (got - fixed).abs() <= band {
            pool.push(e.clone());
        }
    }
    if pool.is_empty() {
        return Err(KernelError::Invalid("the face has no edges along the boundary a match would keep".into()));
    }
    let first = pool.remove(0);
    let mut chain = vec![first];
    let mut end = match chain.first() {
        Some(e) => e.back().clone(),
        None => return Err(KernelError::Failed("match: chain".into())),
    };
    while let Some(k) = pool.iter().position(|e| e.front().id() == end.id() || e.back().id() == end.id()) {
        let e = pool.remove(k);
        let e = if e.front().id() == end.id() { e } else { e.inverse() };
        end = e.back().clone();
        chain.push(e);
    }
    let mut start = match chain.first() {
        Some(e) => e.front().clone(),
        None => return Err(KernelError::Failed("match: chain".into())),
    };
    while let Some(k) = pool.iter().position(|e| e.front().id() == start.id() || e.back().id() == start.id()) {
        let e = pool.remove(k);
        let e = if e.back().id() == start.id() { e } else { e.inverse() };
        start = e.front().clone();
        chain.insert(0, e);
    }
    if !pool.is_empty() {
        return Err(KernelError::Invalid("the face's edges along that boundary do not run on from one another".into()));
    }
    Ok(chain)
}

/// The faces of `target` that meet the chain and whose tangent plane holds the leave direction,
/// best first: the face a match continues from, and the ones it might have been read from.
fn support_faces(target: &Body, chain: &[mt::Edge], leave: Vec3) -> Result<Vec<(mt::Face, f64)>> {
    let mid = chain_mid(chain)?;
    let tol = target.size() * 1e-6 + 1e-9;
    let mut out: Vec<(mt::Face, f64)> = Vec::new();
    for face in target.solid.face_iter() {
        if !chain.iter().all(|e| face.edge_iter().any(|f| f.id() == e.id())) {
            continue;
        }
        let Some((n, d)) = normal_near(face, mid) else { continue };
        if d > tol {
            continue;
        }
        let score = (leave - n * leave.dot(n)).len();
        if score.is_finite() && score >= 0.2 {
            out.push((face.clone(), score));
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
fn supply_order(candidates: &[(mt::Face, f64)], chain: &[mt::Edge], leave: Vec3, size: f64) -> Result<Continuity> {
    let Some((first, best)) = candidates.first() else {
        return Err(KernelError::Invalid("no face of the target holds the picked edges".into()));
    };
    let keep: Vec<&mt::Face> = candidates.iter().filter(|(_, s)| *s >= 0.5 * best).map(|(f, _)| f).collect();
    let mut order = Continuity::G3;
    if keep.len() < 2 {
        return Ok(order);
    }
    let primary: Vec<Frame> = bridge::frames(first, chain, leave, STATIONS)?;
    for other in keep.iter().skip(1) {
        // A face that cannot be read along the whole chain says nothing here: the face the match
        // is built on is the one that must be able to supply it.
        let Ok(other_frames) = bridge::frames(other, chain, leave, STATIONS) else { continue };
        let mut worst_deg = 0.0f64;
        let mut worst = 0.0f64;
        let mut worst_change = 0.0f64;
        for (a, b) in primary.iter().zip(&other_frames) {
            if let (Some((na, _)), Some((nb, _))) = (normal_near(first, a.p), normal_near(other, b.p)) {
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

/// The shape of a face's surface along its cross direction at the station `t`, read inwards from
/// the boundary `near` over five one-sided samples: the point, the unit tangent, the curvature
/// vector and the change of that curvature. The same finite difference [`crate::bridge::frames`]
/// reads a support surface with, so that a rebuilt seam can be compared with what it matched.
fn seam_shape(surf: &mt::Surface, ranges: &[(f64, f64); 2], near: Side, t: f64) -> Result<(Vec3, Vec3, Vec3, Vec3)> {
    let (station, cross, at_start) = if near.is_u() {
        (ranges[1].0 + (ranges[1].1 - ranges[1].0) * t, ranges[0], near == Side::U0)
    } else {
        (ranges[0].0 + (ranges[0].1 - ranges[0].0) * t, ranges[1], near == Side::V0)
    };
    let span = (cross.1 - cross.0).abs().max(1e-12);
    let h = span * 1e-3;
    let c0 = if at_start { cross.0 } else { cross.1 };
    let sgn = if at_start { 1.0 } else { -1.0 };
    let at = |x: f64| if near.is_u() { from_p3(surf.subs(x, station)) } else { from_p3(surf.subs(station, x)) };
    let q: Vec<Vec3> = (0..5).map(|k| at(c0 + sgn * k as f64 * h)).collect();
    let (Some(&q0), Some(&q1), Some(&q2), Some(&q3), Some(&q4)) = (q.first(), q.get(1), q.get(2), q.get(3), q.get(4)) else {
        return Err(KernelError::Failed("match: seam samples".into()));
    };
    let d1 = (q0 * -25.0 + q1 * 48.0 + q2 * -36.0 + q3 * 16.0 + q4 * -3.0) * (sgn / (12.0 * h));
    let d2 = (q0 * 35.0 + q1 * -104.0 + q2 * 114.0 + q3 * -56.0 + q4 * 11.0) * (1.0 / (12.0 * h * h));
    let d3 = (q0 * -5.0 + q1 * 18.0 + q2 * -24.0 + q3 * 14.0 + q4 * -3.0) * (sgn / (2.0 * h * h * h));
    let (t, bend, change) = shape(d1, d2, d3)?;
    Ok((q0, t, bend, change))
}

/// How far the rebuilt surface strays from the original one: the root mean square of the
/// distances from a grid of its samples to the original surface.
fn deviation(made: &mt::Face, original: &mt::Face) -> Result<f64> {
    let (surf, os) = (made.surface(), original.surface());
    let ranges = ranges(made)?;
    let mut sum = 0.0f64;
    let mut n = 0usize;
    for i in 0..GRID {
        for j in 0..GRID {
            let u = ranges[0].0 + (ranges[0].1 - ranges[0].0) * i as f64 / (GRID - 1) as f64;
            let v = ranges[1].0 + (ranges[1].1 - ranges[1].0) * j as f64 / (GRID - 1) as f64;
            let q = from_p3(surf.subs(u, v));
            let Some((a, b)) = os.search_nearest_parameter(p3(q), None, 100) else { continue };
            let d = from_p3(os.subs(a, b)).dist(q);
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
