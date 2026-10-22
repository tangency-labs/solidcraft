//! Ruled surfaces (Surface › Ruled): a strip hanging off a chain of a body's edges, a given
//! width out from them, as a new surface body. Each edge carries one strip, ruled by straight
//! lines from the edge in a direction set by the face beside it: along its normal, continuing
//! it, at an angle from a reference direction (a draft), or along a fixed vector.
//!
//! A strip is exact where the rulings move the edge by an affine map: a straight edge with a
//! constant ruling direction gives a plane, an arc whose rulings keep their angle to its axis
//! gives a plane (a ring), a cylinder or a cone. Any other strip is a ruled B-spline (degree 1
//! across) fitted within [`FIT_TOLERANCE`]. Strips share their side edges where their rulings
//! agree; at a sharp corner of the chain they are mitred (each extended along its far edge
//! until the two meet, or cut back where they overlap) or rounded (a planar or conical sector
//! turning one ruling into the other; an inside corner is always cut back). The strips stand
//! on the body's own edges, so they come out stitched together.

use std::collections::HashMap;

use mt::{BoundedCurve, ParametricCurve, ParametricSurface3D, SearchNearestParameter};
use solidcraft_geom::Vec3;
use truck_modeling as mt;

use crate::body::{Body, from_p3, p3, v3};
use crate::record::{Rebuild, rebuilt_body};
use crate::surfaces::{map_control_points, nearest_face, picked_chain, surface_body};
use crate::{KernelError, Result, guard};

/// How far a fitted strip may stray from the true ruled surface (mm).
const FIT_TOLERANCE: f64 = 1e-3;
/// Samples along an edge when classifying its strip, and the most a fitted strip is refined to.
const SAMPLES: usize = 16;
const MAX_FIT_SAMPLES: usize = 513;
/// Rulings closer than this (as unit vectors) agree: their strips share a side edge.
const SAME_RULING: f64 = 1e-7;

/// The direction a ruled strip leaves its edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RuledMode {
    /// Along the normal of the face beside the edge.
    Normal,
    /// Continuing the face beside the edge, square to the edge.
    Tangent,
    /// `angle` (radians) from `reference` (default: the face's normal), turned away from the
    /// face about the edge: 0 is the reference, 90° square to it.
    Angle { angle: f64, reference: Option<Vec3> },
    /// Along a fixed vector.
    Direction(Vec3),
}

/// How strips meet at a sharp corner of the chain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RuledCorners {
    /// Extended (or cut back) until they meet.
    #[default]
    Mitre,
    /// Joined by a sector turning one ruling into the other (inside corners are cut back).
    Round,
}

/// A ruled surface's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ruled {
    /// How far the strip reaches along its rulings (mm, positive).
    pub distance: f64,
    pub mode: RuledMode,
    /// The strip on the other side: every ruling reversed.
    pub flip: bool,
    pub corners: RuledCorners,
    /// A point on the face the directions are taken from, for the edges it borders; elsewhere
    /// the face beside the most picked edges (the first in face order on a tie).
    pub face: Option<Vec3>,
}

/// A ruled surface hanging off the chain of `body`'s edges picked by the points `at` (open or
/// closed), as one surface body. The history is over `[body]`: each strip is made in place of
/// the edge it stands on, a corner face in place of the two edges meeting there.
pub fn ruled_surface(body: &Body, at: &[Vec3], r: &Ruled) -> Result<Body> {
    body.require_brep("ruled surface")?;
    let d = r.distance;
    if !(d.is_finite() && d > 0.0 && d < 1e6) {
        return Err(KernelError::Invalid("the ruled surface distance must be positive and finite".into()));
    }
    if at.is_empty() || at.len() > 10_000 {
        return Err(KernelError::Invalid("pick the edges of a chain".into()));
    }
    let unit =
        |v: Vec3, what: &str| v.normalized().filter(|u| u.is_finite()).ok_or_else(|| KernelError::Invalid(format!("the {what} has no length")));
    let mode = match r.mode {
        RuledMode::Angle { angle, reference } => {
            if !(angle.is_finite() && angle.abs() <= std::f64::consts::TAU) {
                return Err(KernelError::Invalid("the ruled surface angle must be finite, a full turn at most".into()));
            }
            RuledMode::Angle { angle, reference: reference.map(|v| unit(v, "reference direction")).transpose()? }
        }
        RuledMode::Direction(v) => RuledMode::Direction(unit(v, "direction")?),
        m => m,
    };
    let picked_face = r.face.map(|p| nearest_face(body, p)).transpose()?;
    let eps = 1e-9 * (1.0 + body.size() + d);
    guard("ruled surface", || {
        let (chain, closed) = picked_chain(body, at)?;
        let faces: Vec<&mt::Face> = body.solid.face_iter().collect();
        // The faces beside each chain edge (face index, the face's own use of the edge).
        let mut beside: HashMap<mt::EdgeID, Vec<(usize, mt::Edge)>> = chain.iter().map(|e| (e.id(), Vec::new())).collect();
        for (i, f) in faces.iter().enumerate() {
            for e in f.boundaries().iter().flat_map(|w| w.edge_iter()) {
                if let Some(list) = beside.get_mut(&e.id()) {
                    list.push((i, e.clone()));
                }
            }
        }
        let mut count: HashMap<usize, usize> = HashMap::new();
        for list in beside.values() {
            for (i, _) in list {
                *count.entry(*i).or_default() += 1;
            }
        }
        let empty = Vec::new();
        let around = |e: &mt::Edge| beside.get(&e.id()).unwrap_or(&empty);
        // The face each edge takes its directions from: the picked face where it borders the
        // edge, carried on along the chain to the faces meeting it smoothly (a cylinder's
        // halves); with no picked face beside the chain, the face beside the most edges.
        let n = chain.len();
        let mut chosen: Vec<Option<usize>> = chain.iter().map(|e| around(e).iter().position(|(i, _)| Some(*i) == picked_face)).collect();
        if chosen.iter().all(Option::is_none) {
            chosen = chain
                .iter()
                .map(|e| {
                    let list = around(e);
                    (0..list.len()).max_by(|a, b| {
                        let key = |k: &usize| list.get(*k).map(|(i, _)| (count.get(i).copied().unwrap_or(0), std::cmp::Reverse(*i)));
                        key(a).cmp(&key(b))
                    })
                })
                .collect();
        }
        // Carried on from a chosen neighbour across the vertex they share.
        let normal = |face: usize, p: mt::Point3| faces.get(face).and_then(|f| normal_at(f, from_p3(p)));
        for _ in 0..n {
            let mut changed = false;
            for j in 0..n {
                if chosen.get(j).copied().flatten().is_some() {
                    continue;
                }
                let Some(e) = chain.get(j) else { continue };
                let neighbours = [
                    (j.checked_sub(1).or(closed.then(|| n - 1)), e.front().point()),
                    ((j + 1 < n).then_some(j + 1).or(closed.then_some(0)), e.back().point()),
                ];
                let pick = neighbours.iter().find_map(|(k, p)| {
                    let k = (*k)?;
                    let (face, _) = around(chain.get(k)?).get(chosen.get(k).copied().flatten()?)?;
                    let want = normal(*face, *p)?;
                    let list = around(e);
                    (0..list.len()).max_by(|a, b| {
                        let dot = |k: &usize| list.get(*k).and_then(|(i, _)| normal(*i, *p)).map_or(f64::NEG_INFINITY, |nn| nn.dot(want));
                        dot(a).total_cmp(&dot(b))
                    })
                });
                if let (Some(k), Some(slot)) = (pick, chosen.get_mut(j)) {
                    *slot = Some(k);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let field = Field { mode, flip: r.flip, distance: d };
        let mut strips = Vec::with_capacity(n);
        for (e, k) in chain.iter().zip(&chosen) {
            let (i, own) = k.and_then(|k| around(e).get(k)).ok_or_else(|| KernelError::Invalid("a picked edge has no face beside it".into()))?;
            let face = faces.get(*i).ok_or_else(|| KernelError::Failed("ruled surface: a face".into()))?;
            // The face lies left of its own boundary (seen along its normal); the strip's
            // tangent direction points away from it.
            let face_left = own.orientation() == e.orientation();
            strips.push(Strip::new(e.clone(), face, face_left, &field, eps)?);
        }
        // Joints at the chain's vertices: joint j is where strip j starts (and strip j - 1
        // ends); an open chain has one more, at its far end.
        let joints = (0..n + usize::from(!closed))
            .map(|j| {
                let incoming = if j > 0 {
                    strips.get(j - 1)
                } else if closed {
                    strips.last()
                } else {
                    None
                };
                let outgoing = strips.get(j).filter(|_| closed || j < n);
                Joint::new(incoming, outgoing, &mode, r.corners, eps)
            })
            .collect::<Result<Vec<_>>>()?;
        let mut rec = Rebuild::default();
        let mut out = Vec::new();
        for (j, s) in strips.iter().enumerate() {
            let start = joints.get(j).ok_or_else(|| KernelError::Failed("ruled surface: a joint".into()))?;
            let end = joints.get(if closed { (j + 1) % n } else { j + 1 }).ok_or_else(|| KernelError::Failed("ruled surface: a joint".into()))?;
            let (up_start, top_start) = start.side(Side::Out, s);
            let (up_end, top_end) = end.side(Side::In, s);
            let top = if s.line {
                mt::Edge::new_unchecked(&top_start, &top_end, mt::Curve::Line(mt::Line(top_start.point(), top_end.point())))
            } else {
                mt::Edge::new_unchecked(&top_start, &top_end, s.top.clone())
            };
            if s.line && (from_p3(top_end.point()) - from_p3(top_start.point())).dot(s.start.tangent) <= eps {
                return Err(KernelError::Invalid("the corners cut a strip away entirely: try a shorter distance".into()));
            }
            let mut wire = vec![s.base.clone()];
            wire.extend(up_end.iter().cloned());
            wire.push(top.inverse());
            wire.extend(up_start.iter().rev().map(mt::Edge::inverse));
            let face = mt::Face::new_unchecked(vec![wire.into()], s.surface.clone());
            rec.blend(&face, [s.base.id()]);
            out.push(face);
            out.extend(end.corner_faces().into_iter().cloned());
        }
        // Corner faces are made in place of the two edges meeting at the corner.
        for (j, jt) in joints.iter().enumerate() {
            let near: Vec<mt::EdgeID> = [
                if j > 0 {
                    strips.get(j - 1)
                } else if closed {
                    strips.last()
                } else {
                    None
                },
                strips.get(j),
            ]
            .into_iter()
            .flatten()
            .map(|s| s.base.id())
            .collect();
            for f in jt.corner_faces() {
                rec.blend(f, near.iter().copied());
            }
        }
        Ok(rebuilt_body(surface_body(out), &[&body.solid], &rec))
    })
}

/// How the rulings are set at a point of an edge.
struct Field {
    mode: RuledMode,
    flip: bool,
    distance: f64,
}

impl Field {
    /// The unit ruling at a point where the edge runs along `t` (unit, chain direction), the
    /// face's normal is `n` and `out` (unit) points away from the face, square to the edge.
    fn ruling(&self, t: Vec3, n: Vec3, out: Vec3) -> Result<Vec3> {
        let dir = match self.mode {
            RuledMode::Normal => n,
            RuledMode::Tangent => out,
            RuledMode::Angle { angle, reference } => {
                let r = reference.unwrap_or(n);
                let r = (r - t * r.dot(t)).normalized().ok_or_else(|| KernelError::Invalid("the reference direction runs along an edge".into()))?;
                // Square to the reference and the edge, turned away from the face.
                let mut w = t.cross(r);
                if w.dot(out + n) < 0.0 {
                    w = -w;
                }
                r * angle.cos() + w * angle.sin()
            }
            RuledMode::Direction(v) => v,
        };
        let dir = if self.flip { -dir } else { dir };
        if dir.cross(t).len() < 1e-6 {
            return Err(KernelError::Invalid("the strip would run along an edge: pick another direction".into()));
        }
        Ok(dir)
    }
}

/// One end of a strip, in the chain's direction.
#[derive(Clone, Copy)]
struct End {
    /// The edge's end point, the unit ruling there and where the ruling ends.
    at: Vec3,
    ruling: Vec3,
    top: Vec3,
    /// The unit tangent of the strip's far edge there, and the face's normal.
    tangent: Vec3,
    normal: Vec3,
}

/// The strip on one edge.
struct Strip {
    /// The body's edge, turned to run along the chain.
    base: mt::Edge,
    surface: mt::Surface,
    /// The far edge's curve (from the start's ruling end to the end's).
    top: mt::Curve,
    /// A plane through a straight edge: its far edge is the line between its corners.
    line: bool,
    /// The strip lies in a plane.
    planar: bool,
    start: End,
    end: End,
}

impl Strip {
    fn new(base: mt::Edge, face: &mt::Face, face_left: bool, field: &Field, eps: f64) -> Result<Strip> {
        let c = base.oriented_curve();
        let (t0, t1) = c.range_tuple();
        let d = field.distance;
        // The point, unit tangent, face normal and ruling at a parameter of the edge.
        let at = |t: f64| -> Result<(Vec3, Vec3, Vec3, Vec3)> {
            let p = from_p3(c.subs(t));
            let tan = from_p3(mt::Point3::new(0.0, 0.0, 0.0) + c.der(t))
                .normalized()
                .ok_or_else(|| KernelError::Invalid("a picked edge has a point with no direction".into()))?;
            let n = normal_at(face, p)
                .ok_or_else(|| KernelError::Invalid("the face beside an edge has a point with no normal there: not supported yet".into()))?;
            let out = if face_left { tan.cross(n) } else { n.cross(tan) };
            let out = out.normalized().ok_or_else(|| KernelError::Invalid("the face beside an edge runs along it".into()))?;
            Ok((p, tan, n, field.ruling(tan, n, out)?))
        };
        let param = |k: usize, m: usize| t0 + (t1 - t0) * k as f64 / (m - 1) as f64;
        let samples: Vec<(Vec3, Vec3, Vec3, Vec3)> = (0..SAMPLES).map(|k| at(param(k, SAMPLES))).collect::<Result<_>>()?;
        let (Some(first), Some(last)) = (samples.first().copied(), samples.last().copied()) else {
            return Err(KernelError::Failed("ruled surface: samples".into()));
        };
        let tops: Vec<Vec3> = samples.iter().map(|(p, _, _, r)| *p + *r * d).collect();
        // A plane through the edge and its rulings, when everything lies in one.
        let plane_normal = first.1.cross(first.3).normalized();
        let planar =
            plane_normal.is_some_and(|nn| samples.iter().map(|s| s.0).chain(tops.iter().copied()).all(|q| (q - first.0).dot(nn).abs() <= eps));
        let exact = affine(&samples, d, eps).and_then(|m| map_control_points(&c, |p| m.apply(p)));
        let line = planar && matches!(c, mt::Curve::Line(_)) && exact.is_some();
        let (surface, top) = match (planar, exact) {
            (true, Some(top)) => (plane(first.0, first.1, first.3), top),
            (false, Some(top)) => {
                let s = mt::BSplineSurface::homotopy(c.clone().lift_up(), top.clone().lift_up());
                (mt::Surface::NurbsSurface(mt::NurbsSurface::new(s)), top)
            }
            (planar, None) => {
                let (s, top) = fitted(&at, &param, d)?;
                (if planar { plane(first.0, first.1, first.3) } else { s }, top)
            }
        };
        let (s0, s1) = top.range_tuple();
        let tangent = |s: f64| from_p3(mt::Point3::new(0.0, 0.0, 0.0) + top.der(s)).normalized().unwrap_or(first.1);
        let end = |(p, _, n, r): (Vec3, Vec3, Vec3, Vec3), tan: Vec3| End { at: p, ruling: r, top: p + r * d, tangent: tan, normal: n };
        Ok(Strip { start: end(first, tangent(s0)), end: end(last, tangent(s1)), base, surface, top, line, planar })
    }
}

/// The unit normal of `face` (as it faces) at its point nearest `p`.
fn normal_at(face: &mt::Face, p: Vec3) -> Option<Vec3> {
    let surface = face.oriented_surface();
    let (u, v) = surface.search_nearest_parameter(p3(p), None, 100)?;
    from_p3(mt::Point3::new(0.0, 0.0, 0.0) + surface.normal(u, v)).normalized()
}

/// The plane through `o` spanned by the edge direction `t` and the ruling `r`, its normal t × r
/// (the strip's own orientation).
fn plane(o: Vec3, t: Vec3, r: Vec3) -> mt::Surface {
    mt::Surface::Plane(mt::Plane::new(p3(o), p3(o + t), p3(o + r)))
}

/// An affine map taking each point of an edge to the end of its ruling.
enum Affine {
    Translate(Vec3),
    /// Points scaled by `scale` about the axis through `c` along `a` (in its square plane),
    /// then moved by `shift`.
    Radial {
        c: Vec3,
        a: Vec3,
        scale: f64,
        shift: Vec3,
    },
}

impl Affine {
    fn apply(&self, p: Vec3) -> Vec3 {
        match *self {
            Affine::Translate(v) => p + v,
            Affine::Radial { c, a, scale, shift } => {
                let q = p - c;
                let along = q.dot(a);
                c + (q - a * along) * scale + a * along + shift
            }
        }
    }
}

/// The affine map the rulings (scaled by `d`) make of the sampled edge, when there is one: a
/// constant ruling translates; on a circular arc, rulings keeping their radial and axial parts
/// (and none along the arc) scale about its axis and shift along it.
fn affine(samples: &[(Vec3, Vec3, Vec3, Vec3)], d: f64, eps: f64) -> Option<Affine> {
    let first = samples.first()?;
    if samples.iter().all(|s| (s.3 - first.3).len() * d <= eps) {
        return Some(Affine::Translate(first.3 * d));
    }
    let k = samples.len();
    let (a, b, c) = (samples.get(k / 6)?.0, samples.get(k / 2)?.0, samples.get(k - 1 - k / 6)?.0);
    let (u, v) = (b - a, c - a);
    let w = u.cross(v);
    let ww = w.dot(w);
    if !(ww > 1e-24) {
        return None;
    }
    let centre = a + (v.cross(w) * u.dot(u) + w.cross(u) * v.dot(v)) / (2.0 * ww);
    let axis = w.normalized()?;
    let radius = (a - centre).len();
    let (mut radial, mut along) = (None, None);
    for (p, t, _, r) in samples {
        let q = *p - centre;
        if q.dot(axis).abs() > eps || (q.len() - radius).abs() > eps {
            return None;
        }
        let rho = q.normalized()?;
        let (x, y) = (r.dot(rho) * d, r.dot(axis) * d);
        if (r.dot(*t) * d).abs() > eps || radial.is_some_and(|x0: f64| (x - x0).abs() > eps) || along.is_some_and(|y0: f64| (y - y0).abs() > eps) {
            return None;
        }
        radial.get_or_insert(x);
        along.get_or_insert(y);
    }
    Some(Affine::Radial { c: centre, a: axis, scale: 1.0 + radial? / radius, shift: axis * along? })
}

/// A ruled B-spline fitted to the strip (degree 1 across: cubics through matching samples of
/// the edge and of its rulings' ends), refined until both are within [`FIT_TOLERANCE`]; and its
/// far edge.
fn fitted(at: &dyn Fn(f64) -> Result<(Vec3, Vec3, Vec3, Vec3)>, param: &dyn Fn(usize, usize) -> f64, d: f64) -> Result<(mt::Surface, mt::Curve)> {
    let mut m = SAMPLES + 1;
    loop {
        let pts: Vec<(Vec3, Vec3, Vec3, Vec3)> = (0..m).map(|k| at(param(k, m))).collect::<Result<_>>()?;
        let near: Vec<Vec3> = pts.iter().map(|s| s.0).collect();
        let far: Vec<Vec3> = pts.iter().map(|s| s.0 + s.3 * d).collect();
        let fail = || KernelError::Failed("ruled surface: fitting a strip".into());
        let (base, top) = (crate::build::interpolate_cubic(&near).ok_or_else(fail)?, crate::build::interpolate_cubic(&far).ok_or_else(fail)?);
        // Checked between the samples, where a fit strays most.
        let mut worst = 0.0f64;
        for k in 0..m - 1 {
            let s = (k as f64 + 0.5) / (m - 1) as f64;
            let (p, _, _, r) = at(param(2 * k + 1, 2 * m - 1))?;
            worst = worst.max(from_p3(base.subs(s)).dist(p)).max(from_p3(top.subs(s)).dist(p + r * d));
        }
        if worst <= FIT_TOLERANCE {
            let surface = mt::Surface::BSplineSurface(mt::BSplineSurface::homotopy(base, top.clone()));
            return Ok((surface, mt::Curve::BSplineCurve(top)));
        }
        if m >= MAX_FIT_SAMPLES {
            return Err(KernelError::Invalid(format!("the strip can't be fitted within {FIT_TOLERANCE} mm (off by up to {worst:.4} mm)")));
        }
        m = (2 * m - 1).min(MAX_FIT_SAMPLES);
    }
}

/// Which strip of a joint: the one ending there or the one starting there.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    In,
    Out,
}

/// How the strips meet at a vertex of the chain (or end at a free end of it).
enum Joint {
    /// One side edge from the vertex up the ruling: the strips' rulings agree, or a free end.
    Shared(mt::Edge),
    /// Mitred: the edge from the vertex to where the strips' far edges meet (`m`); a curved
    /// strip keeps its own side edge and gets a triangle (`incoming`, `outgoing`) up to it.
    Mitre { edge: mt::Edge, m: mt::Vertex, incoming: Option<(mt::Edge, mt::Face)>, outgoing: Option<(mt::Edge, mt::Face)> },
    /// Rounded: each strip's own side edge, and the sector turning one into the other (in
    /// pieces of less than a half turn).
    Round { incoming: mt::Edge, outgoing: mt::Edge, sector: Vec<mt::Face> },
}

impl Joint {
    fn new(a: Option<&Strip>, b: Option<&Strip>, mode: &RuledMode, corners: RuledCorners, eps: f64) -> Result<Joint> {
        let side = |s: &Strip, e: &End, from_start: bool| {
            let v = if from_start { s.base.front().clone() } else { s.base.back().clone() };
            let top = mt::Vertex::new(p3(e.top));
            mt::Edge::new_unchecked(&v, &top, mt::Curve::Line(mt::Line(v.point(), top.point())))
        };
        let (a, b) = match (a, b) {
            (Some(a), Some(b)) => (a, b),
            (Some(a), None) => return Ok(Joint::Shared(side(a, &a.end, false))),
            (None, Some(b)) => return Ok(Joint::Shared(side(b, &b.start, true))),
            (None, None) => return Err(KernelError::Failed("ruled surface: an empty joint".into())),
        };
        let (ea, eb) = (&a.end, &b.start);
        let vertex = a.base.back().clone();
        if (ea.ruling - eb.ruling).len() <= SAME_RULING {
            let top = mt::Vertex::new(p3((ea.top + eb.top) * 0.5));
            return Ok(Joint::Shared(mt::Edge::new_unchecked(&vertex, &top, mt::Curve::Line(mt::Line(vertex.point(), top.point())))));
        }
        // Where the far edges, carried on straight, meet: λ along the incoming one past its
        // end, μ along the outgoing one past its start.
        let meet = || -> Option<(Vec3, f64, f64)> {
            let (p, q, u, v) = (ea.top, eb.top, ea.tangent, eb.tangent);
            let w = p - q;
            let (uu, uv, vv, uw, vw) = (u.dot(u), u.dot(v), v.dot(v), u.dot(w), v.dot(w));
            let den = uu * vv - uv * uv;
            if den < 1e-12 {
                return None;
            }
            let (lam, mu) = ((uv * vw - vv * uw) / den, (uu * vw - uv * uw) / den);
            let (x, y) = (p + u * lam, q + v * mu);
            (x.dist(y) <= eps * 100.0 && lam.abs() < 1e3 * (1.0 + ea.at.dist(ea.top))).then_some(((x + y) * 0.5, lam, mu))
        };
        // An inside corner (the strips overlap) is cut back where the far edges meet; that needs
        // straight far edges.
        let inside = eb.ruling.dot(ea.tangent) - ea.ruling.dot(eb.tangent) < -1e-9;
        let corner_at = vertex.point();
        let refuse = |what: &str| KernelError::Invalid(format!("{what} at the corner at {:?}", from_p3(corner_at)));
        if corners == RuledCorners::Mitre || inside {
            let (m, lam, mu) = meet().ok_or_else(|| refuse("the strips' far edges don't meet: try round corners,"))?;
            let convex = lam > 0.0 && mu < 0.0;
            let concave = lam < 0.0 && mu > 0.0;
            if !(convex || concave) {
                return Err(refuse("the strips cross"));
            }
            if concave && !(a.line && b.line) {
                return Err(refuse("not supported yet: cutting back a curved strip"));
            }
            let mv = mt::Vertex::new(p3(m));
            let edge = mt::Edge::new_unchecked(&vertex, &mv, mt::Curve::Line(mt::Line(vertex.point(), mv.point())));
            // A strip that isn't a plane keeps its side edge and gets a triangle up to the
            // mitre, in its tangent plane there.
            let triangle = |s: &Strip, e: &End, from_start: bool| -> Option<(mt::Edge, mt::Face)> {
                if s.planar {
                    return None;
                }
                let own = side(s, e, from_start);
                let top = own.back().clone();
                let rim = mt::Edge::new_unchecked(&mv, &top, mt::Curve::Line(mt::Line(mv.point(), top.point())));
                let (wire, surface): (Vec<mt::Edge>, _) = if from_start {
                    (vec![own.clone(), rim.inverse(), edge.inverse()], plane_through(e.at, e.top, m))
                } else {
                    (vec![edge.clone(), rim, own.inverse()], plane_through(e.at, m, e.top))
                };
                Some((own, mt::Face::new_unchecked(vec![wire.into()], surface)))
            };
            let incoming = triangle(a, ea, false);
            let outgoing = triangle(b, eb, true);
            return Ok(Joint::Mitre { edge, m: mv, incoming, outgoing });
        }
        // Rounded: the incoming ruling turned into the outgoing one about an axis both keep
        // the same angle to (the reference direction, or the faces' normal: a cone), else
        // about the axis square to both (a flat sector).
        let mut axes: Vec<Vec3> = Vec::new();
        if let RuledMode::Angle { reference: Some(r), .. } = mode {
            axes.push(*r);
        }
        if let Some(n) = (ea.normal + eb.normal).normalized() {
            axes.push(n);
        }
        let flat = |x: Vec3, ax: Vec3| x - ax * x.dot(ax);
        let axis = axes
            .into_iter()
            .find(|ax| {
                (ea.ruling.dot(*ax) - eb.ruling.dot(*ax)).abs() <= 1e-9 && flat(ea.ruling, *ax).len() > 1e-6 && flat(eb.ruling, *ax).len() > 1e-6
            })
            .or_else(|| ea.ruling.cross(eb.ruling).normalized())
            .ok_or_else(|| refuse("the strips turn back on each other"))?;
        let (pa, pb) = (flat(ea.ruling, axis), flat(eb.ruling, axis));
        let turn = pa.cross(pb).dot(axis).atan2(pa.dot(pb));
        let (axis, turn) = if turn < 0.0 { (-axis, -turn) } else { (axis, turn) };
        if !(turn > 1e-9 && turn < std::f64::consts::PI - 1e-6) {
            return Err(refuse("the strips turn back on each other"));
        }
        let incoming = side(a, ea, false);
        // Swept about the vertex (on the axis, so the sector closes there in a point).
        let mut sector: Vec<mt::Face> = mt::builder::cone(&vec![incoming.clone()].into(), v3(axis), mt::Rad(turn)).into_iter().collect();
        let edges: Vec<mt::Edge> =
            sector.iter().flat_map(|f| f.boundaries().into_iter().flat_map(|w| w.edge_iter().cloned().collect::<Vec<_>>())).collect();
        // The sector runs down the incoming side (the strip runs up it).
        if edges.iter().any(|e| e.id() == incoming.id() && e.front().id() == vertex.id()) {
            for f in &mut sector {
                f.invert();
            }
        }
        // Its other side: the edge from the vertex that only one piece of it has.
        let uses = |id: mt::EdgeID| edges.iter().filter(|e| e.id() == id).count();
        let outgoing = edges
            .iter()
            .find(|e| e.id() != incoming.id() && uses(e.id()) == 1 && (e.front().id() == vertex.id() || e.back().id() == vertex.id()))
            .map(|e| if e.front().id() == vertex.id() { e.clone() } else { e.inverse() })
            .ok_or_else(|| KernelError::Failed("ruled surface: a round corner's side".into()))?;
        Ok(Joint::Round { incoming, outgoing, sector })
    }

    /// The edges a strip runs up from the joint's vertex (in order), and the vertex its far edge
    /// starts or ends at.
    fn side(&self, which: Side, s: &Strip) -> (Vec<mt::Edge>, mt::Vertex) {
        match self {
            Joint::Shared(e) => (vec![e.clone()], e.back().clone()),
            Joint::Round { incoming, outgoing, .. } => {
                let e = if which == Side::In { incoming } else { outgoing };
                (vec![e.clone()], e.back().clone())
            }
            Joint::Mitre { edge, m, incoming, outgoing } => {
                let own = if which == Side::In { incoming } else { outgoing };
                if let Some((e, _)) = own {
                    return (vec![e.clone()], e.back().clone());
                }
                if s.line {
                    return (vec![edge.clone()], m.clone());
                }
                // A plane under a curved far edge: on along the mitre's far edge to it.
                let e = if which == Side::In { &s.end } else { &s.start };
                let top = mt::Vertex::new(p3(e.top));
                let rim = mt::Edge::new_unchecked(m, &top, mt::Curve::Line(mt::Line(m.point(), top.point())));
                (vec![edge.clone(), rim], top)
            }
        }
    }

    /// The joint's own faces: a round corner's sector, a mitre's triangles.
    fn corner_faces(&self) -> Vec<&mt::Face> {
        match self {
            Joint::Shared(_) => Vec::new(),
            Joint::Round { sector, .. } => sector.iter().collect(),
            Joint::Mitre { incoming, outgoing, .. } => incoming.iter().chain(outgoing).map(|(_, f)| f).collect(),
        }
    }
}

/// The plane through three points, its normal (b − a) × (c − a).
fn plane_through(a: Vec3, b: Vec3, c: Vec3) -> mt::Surface {
    mt::Surface::Plane(mt::Plane::new(p3(a), p3(b), p3(c)))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::f64::consts::{FRAC_1_SQRT_2, PI};

    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};

    use super::*;
    use crate::history::FaceOrigin;
    use crate::{FaceSurface, box_solid, cylinder, face_surface, measure};

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }
    fn ruled(mode: RuledMode, distance: f64, face: Vec3) -> Ruled {
        Ruled { distance, mode, flip: false, corners: RuledCorners::Mitre, face: Some(face) }
    }
    fn area(b: &Body) -> f64 {
        measure(b).unwrap().area
    }
    /// How many faces use each edge of a body.
    fn uses(b: &Body) -> HashMap<mt::EdgeID, usize> {
        let mut m = HashMap::new();
        for e in b.solid.face_iter().flat_map(|f| f.boundaries().into_iter().flat_map(|w| w.edge_iter().map(|e| e.id()).collect::<Vec<_>>())) {
            *m.entry(e).or_default() += 1;
        }
        m
    }
    fn shared(b: &Body) -> usize {
        uses(b).values().filter(|n| **n == 2).count()
    }

    /// A straight edge (a box's top front edge, 10 long) with a strip 4 wide in every mode: a
    /// 10 × 4 plane each time, leaving the edge the way the mode says.
    #[test]
    fn a_straight_edge_in_every_mode() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let (edge, top, front) = ([Vec3::new(5.0, 0.0, 10.0)], Vec3::new(5.0, 5.0, 10.0), Vec3::new(5.0, 0.0, 5.0));
        let (s30, c30) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
        let (s10, c10) = (10f64.to_radians().sin(), 10f64.to_radians().cos());
        let cases: [(Ruled, Vec3, Vec3); 7] = [
            // Up the top's normal.
            (ruled(RuledMode::Normal, 4.0, top), Vec3::new(0.0, 0.0, 10.0), Vec3::new(10.0, 0.0, 14.0)),
            // Out across the top's edge, continuing it.
            (ruled(RuledMode::Tangent, 4.0, top), Vec3::new(0.0, -4.0, 10.0), Vec3::new(10.0, 0.0, 10.0)),
            // 30° from the top's normal, leaning away from it.
            (
                ruled(RuledMode::Angle { angle: 30f64.to_radians(), reference: None }, 4.0, top),
                Vec3::new(0.0, -4.0 * s30, 10.0),
                Vec3::new(10.0, 0.0, 10.0 + 4.0 * c30),
            ),
            // A 10° draft from the pull direction (+Z) off the front face's top edge: out from
            // the part.
            (
                ruled(RuledMode::Angle { angle: 10f64.to_radians(), reference: Some(Vec3::Z) }, 4.0, front),
                Vec3::new(0.0, -4.0 * s10, 10.0),
                Vec3::new(10.0, 0.0, 10.0 + 4.0 * c10),
            ),
            // Along a fixed vector (not a unit one).
            (
                ruled(RuledMode::Direction(Vec3::new(0.0, -3.0, 3.0)), 4.0, top),
                Vec3::new(0.0, -4.0 * FRAC_1_SQRT_2, 10.0),
                Vec3::new(10.0, 0.0, 10.0 + 4.0 * FRAC_1_SQRT_2),
            ),
            // The front face's normal (-Y), and flipped: down the top's normal.
            (ruled(RuledMode::Normal, 4.0, front), Vec3::new(0.0, -4.0, 10.0), Vec3::new(10.0, 0.0, 10.0)),
            (Ruled { flip: true, ..ruled(RuledMode::Normal, 4.0, top) }, Vec3::new(0.0, 0.0, 6.0), Vec3::new(10.0, 0.0, 10.0)),
        ];
        for (r, lo, hi) in cases {
            let s = ruled_surface(&b, &edge, &r).unwrap();
            let m = measure(&s).unwrap();
            assert!(s.is_surface() && s.face_count() == 1, "{r:?}");
            assert!(rel(m.area, 40.0) < 1e-9, "{r:?}: {}", m.area);
            assert!(m.bbox.min.dist(lo) < 1e-9 && m.bbox.max.dist(hi) < 1e-9, "{r:?}: {:?}", m.bbox);
            assert!(matches!(face_surface(&s, 0), Some(FaceSurface::Plane { .. })), "{r:?}");
        }
    }

    /// A circle (a cylinder's top rim, radius 10, picked all round) with a strip 4 wide in every
    /// mode, from the top face: a cylinder 2π·10·4 up its normal, a ring π(14² − 10²) across
    /// it, a cone π·4·(2·10 + 4 sin 30°) at 30°, and the cylinder again along +Z. From the side
    /// face instead, its normal makes the ring and its tangent the cylinder.
    #[test]
    fn a_circle_in_every_mode() {
        let c = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let rim = [Vec3::new(10.0, 0.0, 20.0), Vec3::new(0.0, 10.0, 20.0), Vec3::new(-10.0, 0.0, 20.0), Vec3::new(0.0, -10.0, 20.0)];
        let (top, side) = (Vec3::new(0.0, 0.0, 20.0), Vec3::new(10.0, 0.0, 10.0));
        let cylinder_area = 2.0 * PI * 10.0 * 4.0;
        let ring = PI * (14.0 * 14.0 - 100.0);
        let cases = [
            (ruled(RuledMode::Normal, 4.0, top), cylinder_area, "cylinder"),
            (ruled(RuledMode::Tangent, 4.0, top), ring, "plane"),
            (ruled(RuledMode::Angle { angle: 30f64.to_radians(), reference: None }, 4.0, top), PI * 4.0 * 22.0, "cone"),
            (ruled(RuledMode::Direction(Vec3::new(0.0, 0.0, 2.0)), 4.0, top), cylinder_area, "cylinder"),
            (ruled(RuledMode::Normal, 4.0, side), ring, "plane"),
            (ruled(RuledMode::Tangent, 4.0, side), cylinder_area, "cylinder"),
        ];
        for (r, want, kind) in cases {
            let s = ruled_surface(&c, &rim, &r).unwrap();
            assert!(rel(area(&s), want) < 2e-3, "{r:?}: {} vs {want}", area(&s)); // (measured on meshes)
            for i in 0..s.face_count() {
                let got = face_surface(&s, i);
                let ok = match kind {
                    "cylinder" => matches!(got, Some(FaceSurface::Cylinder { radius, .. }) if (radius - 10.0).abs() < 1e-6),
                    "cone" => matches!(got, Some(FaceSurface::Cone { half_angle, .. }) if (half_angle - 30f64.to_radians()).abs() < 1e-6),
                    _ => matches!(got, Some(FaceSurface::Plane { .. })),
                };
                assert!(ok, "{r:?}: {got:?}");
            }
            // The strips of the rim's arcs share their side edges: the strip closes round.
            assert_eq!(shared(&s), s.face_count(), "{r:?}");
        }
    }

    /// A closed chain round a box top (10 × 10), 2 wide: corners join with no gap or overlap.
    /// Up the normal the strips share their corner edges (area 4·10·2). Across the top, mitred
    /// they make a 14 × 14 frame less the top; rounded, 4·10·2 plus a quarter disc at each
    /// corner. At 45° mitred, four trapezoids 2·(10 + √2); rounded, quarter cones π·√2·2.
    /// Stitching the result sews nothing more: every join is already shared.
    #[test]
    fn a_closed_chain_round_a_box_top() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let rim = [Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0), Vec3::new(5.0, 10.0, 10.0), Vec3::new(0.0, 5.0, 10.0)];
        let top = Vec3::new(5.0, 5.0, 10.0);
        let round = |m: RuledMode| Ruled { corners: RuledCorners::Round, ..ruled(m, 2.0, top) };
        let angle = RuledMode::Angle { angle: PI / 4.0, reference: None };
        let h = 2.0 * FRAC_1_SQRT_2;
        let cases = [
            (ruled(RuledMode::Normal, 2.0, top), 80.0, 4, 4),
            (ruled(RuledMode::Tangent, 2.0, top), 14.0 * 14.0 - 100.0, 4, 4),
            (round(RuledMode::Tangent), 80.0 + PI * 4.0, 12, 12),
            (ruled(angle, 2.0, top), 80.0 + 4.0 * 2.0 * h, 4, 4),
            (round(angle), 80.0 + PI * h * 2.0, 12, 12),
        ];
        for (r, want, faces, joins) in cases {
            let s = ruled_surface(&b, &rim, &r).unwrap();
            assert!(rel(area(&s), want) < 2e-3, "{r:?}: {} vs {want}", area(&s)); // (measured on meshes)
            assert_eq!((s.face_count(), shared(&s)), (faces, joins), "{r:?}");
            // The strips stand on the box's own edges.
            let own: std::collections::HashSet<mt::EdgeID> = b.solid.edge_iter().map(|e| e.id()).collect();
            assert_eq!(uses(&s).keys().filter(|e| own.contains(e)).count(), 4);
            let sewn = crate::stitch(&[&s], 0.0).unwrap();
            assert_eq!((sewn.face_count(), shared(&sewn), uses(&sewn).len()), (faces, joins, uses(&s).len()), "{r:?}");
        }
    }

    /// An inside corner (an L-shaped prism's top: 20 × 10 and 10 × 10) is cut back, mitred or
    /// rounded: across the top 2 wide, the mitred frame is 24 × 24 less a 10 × 10 notch less
    /// the L (476 − 300); rounded, the five outside corners are quarter discs and the inside
    /// one stays mitred (80·2 − 2² + 5·π·2²/4).
    #[test]
    fn an_inside_corner_is_cut_back() {
        let outer = Loop2::polygon(&[
            Vec2::ZERO,
            Vec2::new(20.0, 0.0),
            Vec2::new(20.0, 10.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(10.0, 20.0),
            Vec2::new(0.0, 20.0),
        ]);
        let ell = crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0.0, 10.0).unwrap().pop().unwrap();
        let rim: Vec<Vec3> =
            [(10.0, 0.0), (20.0, 5.0), (15.0, 10.0), (10.0, 15.0), (5.0, 20.0), (0.0, 10.0)].iter().map(|(x, y)| Vec3::new(*x, *y, 10.0)).collect();
        let top = Vec3::new(5.0, 5.0, 10.0);
        let mitre = ruled_surface(&ell, &rim, &ruled(RuledMode::Tangent, 2.0, top)).unwrap();
        assert!(rel(area(&mitre), 176.0) < 1e-9, "{}", area(&mitre));
        let round = ruled_surface(&ell, &rim, &Ruled { corners: RuledCorners::Round, ..ruled(RuledMode::Tangent, 2.0, top) }).unwrap();
        let want = 156.0 + 5.0 * PI;
        assert!(rel(area(&round), want) < 2e-3, "{} vs {want}", area(&round)); // (measured on meshes)
    }

    /// A curved edge a cut left (a box side cut by a cylinder of radius 40: an arc of it on the
    /// side face, 2·40·asin(14/40) long), with strips 3 wide out of that face and continuing
    /// the cut: 3 times its length each.
    #[test]
    fn an_edge_a_cut_left() {
        let b = box_solid(Vec3::new(-14.0, -14.0, 0.0), Vec3::new(14.0, 14.0, 20.0)).unwrap();
        let tool = cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
        let cup = crate::boolean(&b, &tool, crate::BoolOp::Cut).unwrap().unwrap();
        let z = 50.0 - (1600.0f64 - 0.0).sqrt();
        let (rim, side, cut) = ([Vec3::new(0.0, -14.0, z)], Vec3::new(0.0, -14.0, 5.0), Vec3::new(0.0, 0.0, z));
        let want = 3.0 * 80.0 * (14.0f64 / 40.0).asin();
        for r in [ruled(RuledMode::Normal, 3.0, side), ruled(RuledMode::Tangent, 3.0, cut)] {
            let s = ruled_surface(&cup, &rim, &r).unwrap();
            assert!(rel(area(&s), want) < 2e-3, "{r:?}: {} vs {want}", area(&s)); // (measured on meshes)
            let bb = measure(&s).unwrap().bbox;
            assert!((bb.min.y + 17.0).abs() < 1e-6 && (bb.max.y + 14.0).abs() < 1e-6, "{r:?}: {bb:?}");
        }
    }

    /// Off a doubly curved face (a saddle patch over a cup's rim) its normal turns along a
    /// straight edge: the strip is a fitted ruled B-spline, its rulings square to the edge, so
    /// it reaches exactly 3 from the edge's line (within the fit) and covers it end to end.
    #[test]
    fn a_strip_off_a_curved_face_is_fitted() {
        let b = box_solid(Vec3::new(-14.0, -14.0, 0.0), Vec3::new(14.0, 14.0, 20.0)).unwrap();
        let tool = cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
        let cup = crate::boolean(&b, &tool, crate::BoolOp::Cut).unwrap().unwrap();
        let z = |x: f64| 50.0 - (1600.0 - x * x).sqrt();
        let rim = [Vec3::new(14.0, 0.0, z(14.0)), Vec3::new(0.0, 14.0, z(0.0)), Vec3::new(-14.0, 0.0, z(-14.0)), Vec3::new(0.0, -14.0, z(0.0))];
        let cover = crate::patch_edges(&cup, &rim).unwrap();
        let edge = [Vec3::new(14.0, 0.0, z(14.0))];
        let s = ruled_surface(&cover, &edge, &ruled(RuledMode::Normal, 3.0, Vec3::new(0.0, 0.0, 12.0))).unwrap();
        assert!(s.face_count() == 1 && face_surface(&s, 0).is_none(), "{:?}", face_surface(&s, 0));
        let mesh = s.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
        let from_line = |p: &Vec3| Vec3::new(p.x - 14.0, 0.0, p.z - z(14.0)).len();
        let far = mesh.positions.iter().map(from_line).fold(0.0, f64::max);
        assert!((far - 3.0).abs() < 2e-3, "{far}");
        let bb = measure(&s).unwrap().bbox;
        assert!((bb.min.y + 14.0).abs() < 1e-3 && (bb.max.y - 14.0).abs() < 1e-3, "{bb:?}");
    }

    /// Each strip is made in place of the edge it stands on; a corner face in place of the two
    /// edges meeting at it.
    #[test]
    fn strips_come_from_their_edges() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let rim = [Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0)];
        let r = Ruled { corners: RuledCorners::Round, ..ruled(RuledMode::Tangent, 2.0, Vec3::new(5.0, 5.0, 10.0)) };
        let s = ruled_surface(&b, &rim, &r).unwrap();
        let h = s.history().unwrap();
        let edges = |k: usize| h.faces.get(k).unwrap().origins.iter().filter(|o| matches!(o, FaceOrigin::Edge { operand: 0, .. })).count();
        let counts: Vec<usize> = (0..s.face_count()).map(edges).collect();
        assert_eq!(counts.iter().filter(|n| **n == 1).count(), 2, "{counts:?}");
        assert!(counts.iter().all(|n| *n == 1 || *n == 2), "{counts:?}");
    }

    /// Hostile settings are refused, never a panic: a zero, negative or non-finite distance, a
    /// non-finite angle, no direction, a direction along the edge, edges that branch.
    #[test]
    fn hostile_settings_are_refused() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let edge = [Vec3::new(5.0, 0.0, 10.0)];
        let top = Vec3::new(5.0, 5.0, 10.0);
        for d in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e300] {
            assert!(ruled_surface(&b, &edge, &ruled(RuledMode::Normal, d, top)).is_err(), "{d}");
        }
        for m in [
            RuledMode::Angle { angle: f64::NAN, reference: None },
            RuledMode::Angle { angle: 0.5, reference: Some(Vec3::ZERO) },
            RuledMode::Angle { angle: 0.5, reference: Some(Vec3::X) },
            RuledMode::Direction(Vec3::ZERO),
            RuledMode::Direction(Vec3::X),
            RuledMode::Direction(Vec3::new(f64::NAN, 0.0, 1.0)),
        ] {
            assert!(ruled_surface(&b, &edge, &ruled(m, 2.0, top)).is_err(), "{m:?}");
        }
        // Three edges meeting at a corner branch.
        let corner = [Vec3::new(5.0, 0.0, 10.0), Vec3::new(0.0, 5.0, 10.0), Vec3::new(0.0, 0.0, 5.0)];
        assert!(ruled_surface(&b, &corner, &ruled(RuledMode::Normal, 2.0, top)).is_err());
        assert!(ruled_surface(&b, &[], &ruled(RuledMode::Normal, 2.0, top)).is_err());
        assert!(ruled_surface(&b, &[Vec3::new(500.0, 0.0, 0.0)], &ruled(RuledMode::Normal, 2.0, top)).is_err());
    }
}
