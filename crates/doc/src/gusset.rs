//! Attached fabrication gussets, composed from existing planar extrusion operations.
use std::collections::BTreeMap;

use crate::expr::{Kind, Value};
use crate::{DocError, Document, Expr, Feature, FeatureKind, ModelState, PlaneRef, Result};
use serde::{Deserialize, Serialize};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub feature: crate::FeatureId,
    pub line: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
pub struct Support {
    pub plane: PlaneRef,
    pub member: Option<Member>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    #[default]
    Center,
    Positive,
    Negative,
}

fn invalid(s: impl Into<String>) -> DocError {
    DocError::Invalid(s.into())
}

pub fn member(doc: &Document, st: &ModelState, plane: &PlaneRef) -> Result<Option<Member>> {
    let PlaneRef::Face { anchor: Some(anchor), .. } = plane else { return Err(invalid("gusset supports require direct persistent planar faces")) };
    let source = doc.feature(anchor.target.origin_feature).ok_or_else(|| invalid("gusset support producer no longer exists"))?;
    let FeatureKind::FrameMember { names, .. } = &source.kind else { return Ok(None) };
    let ids = crate::frame::member_line_ids(st, &source.kind)?;
    let line = if source.body_names.is_empty() {
        ids.iter().find(|line| names.get(*line).is_some_and(|name| *name == anchor.target.body))
    } else {
        source.body_names.iter().position(|name| *name == anchor.target.body).and_then(|i| ids.get(i))
    }
    .ok_or_else(|| invalid("gusset support is no longer the selected member"))?;
    Ok(Some(Member { feature: source.id.into(), line: line.clone() }))
}

/// A support face as its exact material: the face's own trimmed loops (holes and later cuts
/// included) read from the body's record in the face's plane, with the maps between the source
/// body's frame and the gusset's author frame.
struct Face {
    plane: Plane,
    /// The face's plane in the source body's frame; `regions` are in its coordinates.
    source: Plane,
    regions: Vec<Region2>,
    to_source: crate::Mat,
    to_author: crate::Mat,
    tolerance: f64,
}

/// Loops on one face and pieces in one loop past these are refused rather than read.
const MAX_LOOPS: usize = 4096;
const MAX_PIECES: usize = 65536;

fn face(doc: &Document, st: &ModelState, f: &Feature, support: &Support) -> Result<Face> {
    let PlaneRef::Face { anchor: Some(anchor), .. } = &support.plane else { return Err(invalid("gusset support must be an attached face")) };
    if member(doc, st, &support.plane)? != support.member {
        return Err(invalid("gusset support's member identity changed"));
    }
    let plane = doc.resolve_face_anchor(anchor, f.component, st)?;
    let body = st.body(&anchor.target.body).ok_or_else(|| invalid("gusset support body disappeared"))?;
    if body.body.face_count() > 4096 {
        return Err(invalid("gusset support exceeds the 4096-face limit"));
    }
    let (indices, _) = doc.anchored_face_indices(st, body, &anchor.target)?;
    let to_author = anchor.source_to_author(doc, f.component, body)?;
    let to_source = crate::mat_inverse(&to_author).ok_or_else(|| invalid("gusset support placement is not invertible"))?;
    let tolerance = body.body.planar_edges_tolerance().clamp(1e-9, 1e-3);
    let record = body.body.record();
    let mut source: Option<Plane> = None;
    let mut regions: Vec<Region2> = Vec::new();
    for index in indices {
        let face_plane = body.body.planar_face(index).ok_or_else(|| invalid("gusset support face must be planar"))?;
        // A face split into several record faces is read in the first one's plane, so each of
        // the others must lie in that plane: parallel, and through it, within the tolerance.
        let face_plane = match source {
            None => *source.insert(face_plane),
            Some(first) => {
                let n = first.normal();
                let tilt = face_plane.normal().cross(n).len() * body.body.size().max(1.);
                if !tilt.is_finite() || tilt > tolerance || (face_plane.origin - first.origin).dot(n).abs() > tolerance {
                    return Err(invalid("gusset support's faces don't lie in one plane"));
                }
                first
            }
        };
        let face = record.faces.get(index).ok_or_else(|| invalid("gusset support face disappeared"))?;
        if face.loops.len() > MAX_LOOPS {
            return Err(invalid("gusset support face has too many loops"));
        }
        let mut loops = Vec::new();
        for l in &face.loops {
            // A vertex loop (a pole) bounds no area.
            let solidcore_record::brep::Loop::Edges { edges } = l else { continue };
            let ids: Vec<usize> = edges.iter().map(|e| e.edge as usize).collect();
            let mut segs = Vec::new();
            for pieces in body.body.planar_edges(&face_plane, &ids) {
                let pieces = pieces.ok_or_else(|| invalid("gusset support face has an edge that can't be read exactly"))?;
                for piece in pieces {
                    let crate::kernel::PlanarPiece::Seg(seg) = piece else {
                        return Err(invalid("not supported yet: a gusset on a face bounded by a free-form spline edge"));
                    };
                    segs.push(seg);
                }
                if segs.len() > MAX_PIECES {
                    return Err(invalid("gusset support face has too many edge pieces"));
                }
            }
            loops.push(counter_clockwise(chain(segs, tolerance)?)?);
        }
        // The loop enclosing the most area bounds the material; every other loop is a hole.
        let areas = loops.iter().map(loop_signed_area).collect::<Result<Vec<_>>>()?;
        let outer = areas
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .ok_or_else(|| invalid("gusset support face has no boundary"))?;
        let material = Region2 { outer: loops.swap_remove(outer), holes: Vec::new() };
        let holes: Vec<Region2> = loops.into_iter().map(|outer| Region2 { outer, holes: Vec::new() }).collect();
        let material = if holes.is_empty() {
            vec![material]
        } else {
            crate::kernel::planar_regions_boolean(&[material], &holes, crate::kernel::BoolOp::Cut, tolerance)?
        };
        regions = if regions.is_empty() {
            material
        } else {
            crate::kernel::planar_regions_boolean(&regions, &material, crate::kernel::BoolOp::Union, tolerance)?
        };
    }
    let source = source.ok_or_else(|| invalid("gusset support has no material"))?;
    if regions.is_empty() {
        return Err(invalid("gusset support has no material"));
    }
    Ok(Face { plane, source, regions, to_source, to_author, tolerance })
}

impl Face {
    /// The 2D vector in the face's coordinates whose dot product with a face point gives that
    /// point's position along the author-frame direction `d`, and the constant to add to it.
    fn along(&self, d: Vec3) -> (Vec2, f64) {
        let ds = crate::apply_vector(&self.to_source, d);
        let k = crate::apply_point(&self.to_author, self.source.origin).dot(d);
        (Vec2::new(self.source.x.dot(ds), self.source.y.dot(ds)), k)
    }

    /// The material's least and greatest position along the author-frame direction `d`, from
    /// its exact boundary.
    fn interval(&self, d: Vec3) -> Result<(f64, f64)> {
        let (e, k) = self.along(d);
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for s in self.regions.iter().flat_map(|r| r.loops()).flat_map(|l| &l.segs) {
            let (a, b) = seg_extent(s, e)?;
            lo = lo.min(a);
            hi = hi.max(b);
        }
        if !lo.is_finite() || !hi.is_finite() {
            return Err(invalid("gusset support face has no measurable extent"));
        }
        Ok((lo + k, hi + k))
    }
}

fn seg_ends(s: &Seg2) -> (Vec2, Vec2) {
    match s {
        Seg2::Line { a, b } | Seg2::Conic { a, b, .. } => (*a, *b),
        Seg2::Arc { center, radius, start, sweep } => {
            (*center + Vec2::from_angle(*start) * *radius, *center + Vec2::from_angle(start + sweep) * *radius)
        }
        Seg2::Cubic { p0, p3, .. } => (*p0, *p3),
        Seg2::Bezier { n, p } => (p.first().copied().unwrap_or_default(), p.get(*n as usize).copied().unwrap_or_default()),
    }
}

fn reversed(s: Seg2) -> Seg2 {
    match s {
        Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center, radius, start: start + sweep, sweep: -sweep },
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: p3, p1: p2, p2: p1, p3: p0 },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: b, apex, b: a, w },
        Seg2::Bezier { n, mut p } => {
            if let Some(head) = p.get_mut(..=n as usize) {
                head.reverse();
            }
            Seg2::Bezier { n, p }
        }
    }
}

/// The pieces of one face loop joined end to start. The loop lists its edges in order, but an
/// edge's pieces need not run the loop's way, so each next piece is the one starting or ending
/// where the chain ends, turned to follow it.
fn chain(mut segs: Vec<Seg2>, tolerance: f64) -> Result<Loop2> {
    let close = 10. * tolerance;
    if segs.is_empty() {
        return Err(invalid("gusset support face has an empty loop"));
    }
    let mut out = vec![segs.remove(0)];
    while !segs.is_empty() {
        let end = out.last().map(|s| seg_ends(s).1).unwrap_or_default();
        let next = segs.iter().position(|s| {
            let (a, b) = seg_ends(s);
            a.dist(end) <= close || b.dist(end) <= close
        });
        let Some(i) = next else { return Err(invalid("gusset support face has an open loop")) };
        let s = segs.swap_remove(i);
        out.push(if seg_ends(&s).0.dist(end) <= close { s } else { reversed(s) });
    }
    let ends = out.first().map(|s| seg_ends(s).0).zip(out.last().map(|s| seg_ends(s).1));
    if ends.is_none_or(|(a, b)| a.dist(b) > close) {
        return Err(invalid("gusset support face has an open loop"));
    }
    Ok(Loop2 { segs: out })
}

/// A Bézier-form piece's control points with their weights (1 for a polynomial piece). A
/// rational piece needs positive weights: then it lies in its control points' convex hull, which
/// bounds its extent; any other weight is refused.
fn bezier_form(s: &Seg2) -> Result<Option<Vec<(Vec2, f64)>>> {
    let control = match s {
        Seg2::Cubic { p0, p1, p2, p3 } => [*p0, *p1, *p2, *p3].map(|p| (p, 1.)).to_vec(),
        Seg2::Conic { a, apex, b, w } => vec![(*a, 1.), (*apex, *w), (*b, 1.)],
        Seg2::Bezier { n, p } => {
            if usize::from(*n) > solidcore_record::MAX_BEZIER_DEGREE {
                return Err(invalid("gusset support edge's Bézier degree is past 9"));
            }
            p.iter().take(usize::from(*n) + 1).map(|p| (*p, 1.)).collect()
        }
        Seg2::Line { .. } | Seg2::Arc { .. } => return Ok(None),
    };
    if control.iter().any(|(p, w)| !p.is_finite() || !w.is_finite() || *w <= 0.) {
        return Err(invalid("gusset support edge has a non-finite point or a weight that isn't positive"));
    }
    Ok(Some(control))
}

/// De Casteljau at `t` on control points.
fn casteljau(points: &[Vec2], t: f64) -> Option<Vec2> {
    let mut level = points.to_vec();
    while level.len() > 1 {
        level = level.windows(2).filter_map(|w| Some(w.first()?.lerp(*w.get(1)?, t))).collect();
    }
    level.first().copied()
}

/// The 24 Gauss-Legendre nodes on [0, 1] with their weights: exact for polynomials of degree up
/// to 47. A polynomial piece of degree n (at most 9) has `p × p'` of degree 2n - 1 ≤ 17, so its
/// area integral is exact up to rounding.
fn gauss_24() -> impl Iterator<Item = (f64, f64)> {
    const X: &[f64] = &[
        0.0640568928626056,
        0.1911188674736163,
        0.3150426796961634,
        0.4337935076260451,
        0.5454214713888396,
        0.6480936519369755,
        0.7401241915785544,
        0.820001985973903,
        0.8864155270044011,
        0.9382745520027328,
        0.9747285559713095,
        0.9951872199970214,
    ];
    const W: &[f64] = &[
        0.1279381953467522,
        0.1258374563468283,
        0.1216704729278034,
        0.1155056680537256,
        0.1074442701159656,
        0.0976186521041139,
        0.0861901615319533,
        0.0733464814110803,
        0.0592985849154368,
        0.0442774388174198,
        0.0285313886289337,
        0.0123412297999872,
    ];
    X.iter().zip(W).flat_map(|(&x, &w)| [(0.5 - 0.5 * x, 0.5 * w), (0.5 + 0.5 * x, 0.5 * w)])
}

/// The area between a rational quadratic's chord and the curve, as a share of its control
/// triangle, for weight `w > 0`: an ellipse arc (w < 1, w = cos θ) encloses
/// `cos θ (θ - sin θ cos θ) / sin³ θ` of it, a parabola (w = 1) 2/3, a hyperbola (w = cosh φ)
/// `cosh φ (sinh φ cosh φ - φ) / sinh³ φ`. All three are exact affine invariants; near w = 1
/// the two-term series `2/3 - 4/15 (1 - w)` replaces the cancelling closed forms (its error,
/// O((1 - w)²), is below 1e-8 there).
pub(crate) fn conic_share(w: f64) -> Result<f64> {
    if !w.is_finite() || w <= 0. {
        return Err(invalid("gusset support edge has a conic weight that isn't positive"));
    }
    let share = if (1. - w).abs() < 1e-4 {
        2. / 3. - 4. / 15. * (1. - w)
    } else if w < 1. {
        let (theta, sin) = (w.acos(), (1. - w * w).sqrt());
        w * (theta - sin * w) / (sin * sin * sin)
    } else {
        let (phi, sinh) = (w.acosh(), (w * w - 1.).sqrt());
        w * (sinh * w - phi) / (sinh * sinh * sinh)
    };
    if !share.is_finite() {
        return Err(invalid("gusset support edge's conic can't be measured"));
    }
    Ok(share)
}

/// Twice the Green's-theorem integral `½ ∮ p × dp` over one exact piece: lines and arcs in
/// closed form, a conic as its chord plus the exact share of its control triangle, a polynomial
/// Bézier piece by [`gauss_24`] on its hodograph (exact). Never zero for a piece it can't read.
fn seg_area(s: &Seg2) -> Result<f64> {
    let (a, b) = seg_ends(s);
    let area = match s {
        Seg2::Line { .. } => 0.5 * a.cross(b),
        Seg2::Arc { center, radius, sweep, .. } => 0.5 * (radius * radius * sweep + center.cross(b - a)),
        Seg2::Conic { apex, w, .. } => 0.5 * a.cross(b) + 0.5 * (*apex - a).cross(b - a) * conic_share(*w)?,
        Seg2::Cubic { .. } | Seg2::Bezier { .. } => {
            let control: Vec<Vec2> = bezier_form(s)?.unwrap_or_default().into_iter().map(|(p, _)| p).collect();
            let n = control.len().checked_sub(1).filter(|n| *n >= 1).ok_or_else(|| invalid("gusset support edge has an empty Bézier piece"))?;
            let hodograph: Vec<Vec2> = control.windows(2).filter_map(|w| Some((*w.get(1)? - *w.first()?) * n as f64)).collect();
            let mut sum = 0.;
            for (t, weight) in gauss_24() {
                let (p, d) = casteljau(&control, t).zip(casteljau(&hodograph, t)).ok_or_else(|| invalid("gusset support edge can't be evaluated"))?;
                sum += weight * p.cross(d);
            }
            0.5 * sum
        }
    };
    if !area.is_finite() {
        return Err(invalid("gusset support edge can't be measured"));
    }
    Ok(area)
}

/// The signed area a loop encloses (positive counter-clockwise), from its exact pieces.
pub(crate) fn loop_signed_area(l: &Loop2) -> Result<f64> {
    l.segs.iter().map(seg_area).sum()
}

pub(crate) fn counter_clockwise(l: Loop2) -> Result<Loop2> {
    Ok(if loop_signed_area(&l)? >= 0. { l } else { Loop2 { segs: l.segs.into_iter().rev().map(reversed).collect() } })
}

fn region_area(r: &Region2) -> Result<f64> {
    let mut area = loop_signed_area(&r.outer)?.abs();
    for h in &r.holes {
        area -= loop_signed_area(h)?.abs();
    }
    Ok(area)
}

/// How closely a Bézier piece's extent is resolved, mm: well inside the contact check's
/// tolerance, so the joint's placement is never off by an unresolved bulge.
const EXTENT_TOLERANCE: f64 = 1e-9;

/// The least and greatest of `e · p` over the exact piece `s`: a line's at its ends, an arc's
/// also where its tangent is perpendicular to `e`, a Bézier piece's from [`scalar_extent`].
pub(crate) fn seg_extent(s: &Seg2, e: Vec2) -> Result<(f64, f64)> {
    let (a, b) = seg_ends(s);
    let (mut lo, mut hi) = (a.dot(e).min(b.dot(e)), a.dot(e).max(b.dot(e)));
    match s {
        Seg2::Line { .. } => {}
        Seg2::Arc { center, radius, start, sweep } => {
            let (t0, t1) = if *sweep >= 0. { (*start, start + sweep) } else { (start + sweep, *start) };
            for extreme in [e.angle(), e.angle() + std::f64::consts::PI] {
                let turns = ((t0 - extreme) / std::f64::consts::TAU).ceil();
                if extreme + turns * std::f64::consts::TAU <= t1 {
                    let v = (*center + Vec2::from_angle(extreme) * *radius).dot(e);
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
        Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => {
            let control = bezier_form(s)?.unwrap_or_default();
            let (l, h) = scalar_extent(control.iter().map(|(p, w)| (p.dot(e) * w, *w)).collect(), 24)?;
            lo = lo.min(l);
            hi = hi.max(h);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return Err(invalid("gusset support edge can't be measured"));
    }
    Ok((lo, hi))
}

/// The extent of a 1D rational Bézier with homogeneous control values `(w·v, w)`, every `w > 0`
/// ([`bezier_form`]). With positive weights the curve lies within its control values, and
/// within its end values once no control value is past them; halving narrows the gap. Each
/// half is accepted when its control values are within [`EXTENT_TOLERANCE`] of its end values,
/// so the result is the true extent to within that; a piece not resolved in `depth` halvings
/// is an error, never its unrefined hull.
pub(crate) fn scalar_extent(h: Vec<(f64, f64)>, depth: u32) -> Result<(f64, f64)> {
    if h.iter().any(|(v, w)| !v.is_finite() || !w.is_finite() || *w <= 0.) {
        return Err(invalid("gusset support edge has a weight that isn't positive"));
    }
    let value = |p: &(f64, f64)| p.0 / p.1;
    let (lo, hi) = h.iter().map(value).fold((f64::INFINITY, f64::NEG_INFINITY), |(l, u), v| (l.min(v), u.max(v)));
    let (first, last) = (h.first().map(value), h.last().map(value));
    let (Some(first), Some(last)) = (first, last) else { return Err(invalid("gusset support edge has an empty Bézier piece")) };
    let (elo, ehi) = (first.min(last), first.max(last));
    if lo >= elo - EXTENT_TOLERANCE && hi <= ehi + EXTENT_TOLERANCE {
        return Ok((elo.min(lo.max(elo - EXTENT_TOLERANCE)), ehi.max(hi.min(ehi + EXTENT_TOLERANCE))));
    }
    let Some(depth) = depth.checked_sub(1) else {
        return Err(invalid("gusset support edge's extent couldn't be resolved"));
    };
    let (mut left, mut right) = (Vec::with_capacity(h.len()), Vec::with_capacity(h.len()));
    let mut level = h;
    while let (Some(f), Some(l)) = (level.first().copied(), level.last().copied()) {
        left.push(f);
        right.push(l);
        level = level.windows(2).filter_map(|w| Some(((w.first()?.0 + w.get(1)?.0) * 0.5, (w.first()?.1 + w.get(1)?.1) * 0.5))).collect();
    }
    right.reverse();
    let (l1, h1) = scalar_extent(left, depth)?;
    let (l2, h2) = scalar_extent(right, depth)?;
    Ok((l1.min(l2), h1.max(h2)))
}

fn length(v: f64, positive: bool) -> Result<f64> {
    if !v.is_finite() || v.abs() > 1e6 || (positive && v < 0.01) {
        return Err(invalid("gusset sizes must be 0.01…1e6 mm; offsets must be finite and within 1e6 mm"));
    }
    Ok(v)
}

/// Refuses a contact strip (`from..to` along `leg`, `start..end` along `direction`, from
/// `origin`, in the author frame) that isn't entirely on the face's material: what is left of
/// the strip after cutting the face's exact region away must be empty, so a hole or notch
/// between the strip's corners is caught, not just its corners.
fn contact(face: &Face, origin: Vec3, leg: Vec3, direction: Vec3, from: f64, to: f64, start: f64, end: f64) -> Result<()> {
    let corner = |a: f64, b: f64| face.source.to_local(crate::apply_point(&face.to_source, origin + leg * a + direction * b));
    let pts = [corner(from, start), corner(to, start), corner(to, end), corner(from, end)];
    let segs = pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| Seg2::Line { a: *a, b: *b }).collect();
    let strip = Region2 { outer: counter_clockwise(Loop2 { segs })?, holes: Vec::new() };
    let left = crate::kernel::planar_regions_boolean(&[strip], &face.regions, crate::kernel::BoolOp::Cut, face.tolerance)?;
    let uncovered = left.iter().map(region_area).sum::<Result<f64>>()?;
    let expected = (to - from) * (end - start);
    if !uncovered.is_finite() || uncovered > expected * 1e-6 + 1e-8 {
        return Err(invalid("gusset contact must fit entirely on both selected faces; reduce the legs/thickness or change the joint offset"));
    }
    Ok(())
}

pub(crate) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FrameGusset { supports, leg1, leg2, thickness, relief, clip, offset, placement } = &f.kind else {
        return Err(invalid("not a gusset"));
    };
    let [a, b] = supports.as_slice() else { return Err(invalid("a gusset requires exactly two supports")) };
    if a.plane == b.plane {
        return Err(invalid("select two different support faces"));
    }
    let (a, b) = (face(doc, st, f, a)?, face(doc, st, f, b)?);
    let value = |e: &Expr, positive| length(crate::eval::val(vals, e, Kind::Length)?, positive);
    let (l1, l2, t) = (value(leg1, true)?, value(leg2, true)?, value(thickness, true)?);
    let (r, c, offset) = (value(relief, false)?, value(clip, false)?, value(offset, false)?);
    if r < 0. || c < 0. || r + c >= l1 || r + c >= l2 {
        return Err(invalid("relief and clipping must be nonnegative and together shorter than each leg"));
    }
    let (n1, n2) = (a.plane.normal(), b.plane.normal());
    let dot = n1.dot(n2);
    let denom = 1. - dot * dot;
    if !denom.is_finite() || denom < 1e-8 {
        return Err(invalid("gusset supports must not be parallel or nearly parallel"));
    }
    let direction = n1.cross(n2).normalized().ok_or_else(|| invalid("gusset joint has no direction"))?;
    let u = (n2 - n1 * dot).normalized().ok_or_else(|| invalid("gusset first leg is degenerate"))?;
    let v = (n1 - n2 * dot).normalized().ok_or_else(|| invalid("gusset second leg is degenerate"))?;
    let (h1, h2) = (n1.dot(a.plane.origin), n2.dot(b.plane.origin));
    let mut origin = n1 * ((h1 - dot * h2) / denom) + n2 * ((h2 - dot * h1) / denom);
    let (ia, ib) = (a.interval(direction)?, b.interval(direction)?);
    let (lo, hi) = (ia.0.max(ib.0), ia.1.min(ib.1));
    if hi - lo < 0.01 {
        return Err(invalid("support faces have no common joint span"));
    }
    origin += direction * ((lo + hi) * 0.5 - origin.dot(direction) + offset);
    if !origin.is_finite() || origin.len() > 1e9 {
        return Err(invalid("gusset joint exceeds the coordinate limit"));
    }
    let (start, end) = match placement {
        Placement::Center => (-t / 2., t / 2.),
        Placement::Positive => (0., t),
        Placement::Negative => (-t, 0.),
    };
    contact(&a, origin, u, direction, r, l1 - c, start, end)?;
    contact(&b, origin, v, direction, r, l2 - c, start, end)?;
    let plane = Plane::new(origin, u, direction.cross(u)).ok_or_else(|| invalid("gusset plane is degenerate"))?;
    let (p1, p2) = (u * l1, v * l2);
    let diagonal = p2 - p1;
    let distance = diagonal.len();
    if distance <= 2. * c {
        return Err(invalid("outer clipping consumes the gusset diagonal"));
    }
    let mut points = vec![u * r];
    if c > 0. {
        points.extend([p1 - u * c, p1 + diagonal * (c / distance), p2 - diagonal * (c / distance), p2 - v * c]);
    } else {
        points.extend([p1, p2]);
    }
    if r > 0. {
        points.push(v * r);
    }
    let mut points: Vec<Vec2> = points.into_iter().map(|p| Vec2::new(p.dot(plane.x), p.dot(plane.y))).collect();
    // The ordered support normals define thickness direction, independently of outline winding.
    points.reverse();
    let mut outer = Loop2::polygon(&points);
    if outer.signed_area() < 0. {
        outer = outer.reversed();
    }
    if outer.signed_area() < 1e-6 {
        return Err(invalid("gusset profile has no usable area"));
    }
    let mut bodies = solidcraft_kernel::extrude(&plane, &[Region2 { outer, holes: Vec::new() }], start, end)?;
    let body = bodies.pop().filter(|_| bodies.is_empty()).ok_or_else(|| invalid("gusset must be one solid"))?;
    let name = crate::eval::new_name(st, f, 0);
    st.bodies.push(crate::ModelBody::traced_new(name, crate::lineage::numbered_tool(body), f.id));
    Ok(())
}
