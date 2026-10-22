//! Ruled surfaces (Surface › Ruled): a strip hanging off a chain of a body's edges, a given
//! width out from them, as a new surface body. Each edge carries one strip, ruled by straight
//! lines from the edge in a direction set by the face beside it: along its normal, continuing
//! it, at an angle from a reference direction (a draft), or along a fixed vector.
//!
//! A strip is exact where the rulings move the edge by an affine map: a straight edge with a
//! constant ruling direction gives a plane, an arc whose rulings keep their angle to its axis
//! gives a plane (a ring), or a ruled NURBS surface between the edge and its image (a cylinder
//! or a cone, exactly). Any other strip is a ruled B-spline (degree 1 across) fitted within
//! [`FIT_TOLERANCE`]. Strips share their side edges where their rulings agree; at a sharp
//! corner of the chain they are mitred (each extended along its far edge until the two meet, or
//! cut back where they overlap) or rounded (a planar or conical sector SMLib revolves from one
//! ruling to the other; an inside corner is always cut back).
//!
//! Every strip and corner face is built on its own on the record (each edge a NURBS curve, the
//! edge's own curve as a NURBS where it stands on the body), and the faces are stitched by SMLib
//! along the side edges they share.

use std::collections::HashMap;

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::SurfaceEval;
use solidcraft_geom::brep::{self as b, Brep};
use solidcraft_geom::bspline::{CurveN, HPoint, NurbsSurface};

use crate::body::Body;
use crate::history::FaceOrigin;
use crate::surfaces::{Parts, nearest_face, picked_chain, sheet_of, spline_curve, spline_surface, uv_line};
use crate::{KernelError, Result, record};

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
    let rec = body.record();
    let (chain, closed) = picked_chain(body, at)?;
    let n = chain.len();
    // The faces beside each chain edge: each use's face and whether that face's loop runs the
    // edge the way the chain does.
    let around: Vec<Vec<(usize, bool)>> = chain
        .iter()
        .map(|&(e, forward)| {
            let edge = rec.edges.get(e).ok_or_else(|| KernelError::Invalid("an edge reference".into()))?;
            Ok(edge
                .uses
                .iter()
                .filter_map(|u| {
                    let face = rec.faces.get(u.face as usize)?;
                    let b::Loop::Edges { edges } = face.loops.get(u.r#loop as usize)? else { return None };
                    Some((u.face as usize, edges.get(u.position as usize)?.forward == forward))
                })
                .collect())
        })
        .collect::<Result<_>>()?;
    let mut count: HashMap<usize, usize> = HashMap::new();
    for list in &around {
        for (i, _) in list {
            *count.entry(*i).or_default() += 1;
        }
    }
    let surfaces: Vec<Option<SurfaceEval>> = rec.faces.iter().map(|f| SurfaceEval::new(&f.surface).ok()).collect();
    let normal = |face: usize, p: Vec3| normal_at(surfaces.get(face)?.as_ref()?, body.face_forward(face), p);
    // The face each edge takes its directions from: the picked face where it borders the edge,
    // carried on along the chain to the faces meeting it smoothly (a cylinder's halves); with no
    // picked face beside the chain, the face beside the most edges.
    let mut chosen: Vec<Option<usize>> = around.iter().map(|list| list.iter().position(|(i, _)| Some(*i) == picked_face)).collect();
    if chosen.iter().all(Option::is_none) {
        chosen = around
            .iter()
            .map(|list| {
                (0..list.len()).max_by(|a, b| {
                    let key = |k: &usize| list.get(*k).map(|(i, _)| (count.get(i).copied().unwrap_or(0), std::cmp::Reverse(*i)));
                    key(a).cmp(&key(b))
                })
            })
            .collect();
    }
    let bases: Vec<CurveN<4>> = chain.iter().map(|&(e, forward)| base_curve(rec, e, forward)).collect::<Result<_>>()?;
    let ends = |k: usize| -> Option<(Vec3, Vec3)> { Some((point(bases.get(k)?, 0.0)?, point(bases.get(k)?, 1.0)?)) };
    // Carried on from a chosen neighbour across the vertex they share.
    for _ in 0..n {
        let mut changed = false;
        for j in 0..n {
            if chosen.get(j).copied().flatten().is_some() {
                continue;
            }
            let Some((start, end)) = ends(j) else { continue };
            let neighbours = [(j.checked_sub(1).or(closed.then(|| n - 1)), start), ((j + 1 < n).then_some(j + 1).or(closed.then_some(0)), end)];
            let pick = neighbours.iter().find_map(|(k, p)| {
                let k = (*k)?;
                let (face, _) = around.get(k)?.get(chosen.get(k).copied().flatten()?)?;
                let want = normal(*face, *p)?;
                let list = around.get(j)?;
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
    for (j, k) in chosen.iter().enumerate() {
        let (face, along) =
            k.and_then(|k| around.get(j)?.get(k).copied()).ok_or_else(|| KernelError::Invalid("a picked edge has no face beside it".into()))?;
        let surface = surfaces.get(face).and_then(Option::as_ref).ok_or_else(|| KernelError::Failed("ruled surface: a face's surface".into()))?;
        // The face lies left of its own boundary seen along its (oriented) normal; the strip's
        // tangent direction points away from it.
        let forward = body.face_forward(face);
        let base = bases.get(j).ok_or_else(|| KernelError::Failed("ruled surface: an edge".into()))?;
        strips.push(Strip::new(base.clone(), surface, forward, along == forward, &field, eps)?);
    }
    // Joints at the chain's vertices: joint j is where strip j starts (and strip j - 1 ends); an
    // open chain has one more, at its far end.
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
    let tolerance = (eps * 1e3).max(rec.tolerance);
    let mut pieces = Vec::new();
    for (j, s) in strips.iter().enumerate() {
        let start = joints.get(j).ok_or_else(|| KernelError::Failed("ruled surface: a joint".into()))?;
        let end = joints.get(if closed { (j + 1) % n } else { j + 1 }).ok_or_else(|| KernelError::Failed("ruled surface: a joint".into()))?;
        let (up_start, top_start) = start.side(Side::Out, s);
        let (up_end, top_end) = end.side(Side::In, s);
        if s.line && (top_end - top_start).dot(s.start.tangent) <= eps {
            return Err(KernelError::Invalid("the corners cut a strip away entirely: try a shorter distance".into()));
        }
        let edge = chain.get(j).map_or(0, |(e, _)| *e);
        pieces
            .push((s.face(&up_start, &up_end, top_start, top_end, tolerance)?, vec![record::generated(vec![FaceOrigin::Edge { operand: 0, edge }])]));
    }
    // Corner faces are made in place of the two edges meeting at the corner.
    for (j, jt) in joints.iter().enumerate() {
        let before = if j > 0 {
            Some(j - 1)
        } else if closed {
            n.checked_sub(1)
        } else {
            None
        };
        let near: Vec<FaceOrigin> = [before, (j < n).then_some(j)]
            .into_iter()
            .flatten()
            .filter_map(|k| chain.get(k))
            .map(|(e, _)| FaceOrigin::Edge { operand: 0, edge: *e })
            .collect();
        for f in jt.corner_faces(tolerance)? {
            let h = vec![record::generated(near.clone()); f.faces.len()];
            pieces.push((f, h));
        }
    }
    sheet_of(pieces, true, &[body])
}

/// The unit normal of a face (its surface, used along its natural normal when `forward`) at its
/// surface's point nearest `p`.
fn normal_at(surface: &SurfaceEval, forward: bool, p: Vec3) -> Option<Vec3> {
    let [u, v] = surface.project(p, None, f64::INFINITY)?;
    let (_, su, sv) = surface.derivs(u, v)?;
    let n = su.cross(sv).normalized()?;
    Some(if forward { n } else { n * -1.0 })
}

/// The edge `e` of `r` as a NURBS curve with homogeneous control points, run the way the chain
/// runs along it (`forward`), its parameter the curve's own (an arc's rational quadratic form,
/// one span per quarter turn or less).
fn base_curve(r: &Brep, e: usize, forward: bool) -> Result<CurveN<4>> {
    let edge = r.edges.get(e).ok_or_else(|| KernelError::Invalid("an edge reference".into()))?;
    let [t0, t1] = edge.interval;
    let c = match &edge.curve {
        b::Curve::Line { start, end } => {
            let at = |t: f64| [start[0] + (end[0] - start[0]) * t, start[1] + (end[1] - start[1]) * t, start[2] + (end[2] - start[2]) * t, 1.0];
            CurveN { degree: 1, knots: vec![t0, t0, t1, t1], ctrl: vec![at(t0), at(t1)] }
        }
        b::Curve::Circle { frame, radius } => conic(frame, [*radius, *radius], [t0, t1]),
        b::Curve::Ellipse { frame, radii } => conic(frame, *radii, [t0, t1]),
        b::Curve::Polyline { points } => {
            let n = points.len();
            let mut knots = vec![0.0];
            knots.extend((0..n).map(|i| i as f64));
            knots.push(n.saturating_sub(1) as f64);
            CurveN { degree: 1, knots, ctrl: points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect() }
        }
        b::Curve::Spline { knots, points } => {
            CurveN { degree: degree(knots, points.len())?, knots: knots.clone(), ctrl: points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect() }
        }
        b::Curve::RationalSpline { knots, homogeneous } => {
            CurveN { degree: degree(knots, homogeneous.len())?, knots: knots.clone(), ctrl: homogeneous.clone() }
        }
        b::Curve::SurfaceIntersection { .. } => {
            return Err(KernelError::Invalid("not supported yet: a ruled surface on a traced intersection edge".into()));
        }
    };
    Ok(if forward { c } else { reversed(&c) })
}

fn degree(knots: &[f64], n: usize) -> Result<usize> {
    knots.len().checked_sub(n + 1).filter(|d| *d >= 1).ok_or_else(|| KernelError::Invalid("a spline's knots".into()))
}

/// The arc of a circle or ellipse (its frame and radii) from angle `t0` to `t1` as a rational
/// quadratic on [t0, t1]: the same points, not the same parameterisation.
fn conic(frame: &b::Frame, radii: [f64; 2], [t0, t1]: [f64; 2]) -> CurveN<4> {
    let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
    let (o, x, y) = (v(frame.origin), v(frame.x_axis), v(frame.y_axis));
    let m = ((t1 - t0) / std::f64::consts::FRAC_PI_2 - 1e-9).ceil().max(1.0) as usize;
    let step = (t1 - t0) / m as f64;
    let w = (step / 2.0).cos();
    let at = |a: f64, s: f64| o + x * (radii[0] * a.cos() * s) + y * (radii[1] * a.sin() * s);
    let h = |p: Vec3, w: f64| [p.x * w, p.y * w, p.z * w, w];
    let (mut ctrl, mut knots) = (vec![h(at(t0, 1.0), 1.0)], vec![t0; 3]);
    for j in 0..m {
        let a0 = t0 + step * j as f64;
        ctrl.push(h(at(a0 + step / 2.0, 1.0 / w), w));
        ctrl.push(h(at(a0 + step, 1.0), 1.0));
        let u = t0 + step * (j + 1) as f64;
        if j + 1 < m {
            knots.extend([u, u]);
        } else {
            knots.extend([t1; 3]);
        }
    }
    CurveN { degree: 2, knots, ctrl }
}

/// `c` run the other way: the same points, its parameter t taken to (first + last knot) − t.
fn reversed(c: &CurveN<4>) -> CurveN<4> {
    let (a, z) = (c.knots.first().copied().unwrap_or(0.0), c.knots.last().copied().unwrap_or(1.0));
    CurveN { degree: c.degree, knots: c.knots.iter().rev().map(|k| a + z - k).collect(), ctrl: c.ctrl.iter().rev().copied().collect() }
}

/// A curve's parameter range.
fn range(c: &CurveN<4>) -> [f64; 2] {
    [c.knots.get(c.degree).copied().unwrap_or(0.0), c.knots.get(c.ctrl.len()).copied().unwrap_or(1.0)]
}

/// The point `f` (0…1) of the way along a curve's parameter range.
fn point(c: &CurveN<4>, f: f64) -> Option<Vec3> {
    point_tangent(c, f).map(|(p, _)| p)
}

/// The point `f` of the way along a curve's range and its derivative there (by the parameter).
fn point_tangent(c: &CurveN<4>, f: f64) -> Option<(Vec3, Vec3)> {
    let [t0, t1] = range(c);
    let ([x, y, z, w], [dx, dy, dz, dw]) = c.eval(t0 + (t1 - t0) * f)?;
    if !(w.abs() > 1e-300) {
        return None;
    }
    let p = Vec3::new(x / w, y / w, z / w);
    Some((p, (Vec3::new(dx, dy, dz) - p * dw) * (1.0 / w)))
}

/// `c` with its control points (Cartesian) moved by `f`, its weights kept: the exact image of the
/// curve under an affine `f`.
fn mapped(c: &CurveN<4>, f: impl Fn(Vec3) -> Vec3) -> CurveN<4> {
    let ctrl = c
        .ctrl
        .iter()
        .map(|h| {
            let q = f(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]));
            [q.x * h[3], q.y * h[3], q.z * h[3], h[3]]
        })
        .collect();
    CurveN { degree: c.degree, knots: c.knots.clone(), ctrl }
}

fn hpoints(c: &CurveN<4>) -> Vec<HPoint> {
    c.ctrl.iter().map(|h| HPoint { x: h[0], y: h[1], z: h[2], w: h[3] }).collect()
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
                    w = w * -1.0;
                }
                r * angle.cos() + w * angle.sin()
            }
            RuledMode::Direction(v) => v,
        };
        let dir = if self.flip { dir * -1.0 } else { dir };
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

/// What a strip's surface is.
enum Kind {
    /// The plane through a point spanned by the edge's direction and the ruling (normal t × r).
    Plane { o: Vec3, t: Vec3, r: Vec3 },
    /// The ruled NURBS surface from the base curve (v = 0) to the top (v = 1) on their shared
    /// knots and weights.
    Ruled,
}

/// The strip on one edge.
struct Strip {
    /// The edge as a NURBS curve run along the chain (or its fit, for a fitted strip).
    base: CurveN<4>,
    /// The far edge's curve (from the start's ruling end to the end's), on the base's knots.
    top: CurveN<4>,
    kind: Kind,
    /// A plane through a straight edge: its far edge is the line between its corners.
    line: bool,
    /// The strip lies in a plane.
    planar: bool,
    start: End,
    end: End,
}

impl Strip {
    fn new(base: CurveN<4>, face: &SurfaceEval, forward: bool, face_left: bool, field: &Field, eps: f64) -> Result<Strip> {
        let d = field.distance;
        // The point, unit tangent, face normal and ruling a fraction of the way along the edge.
        let at = |f: f64| -> Result<(Vec3, Vec3, Vec3, Vec3)> {
            let (p, der) = point_tangent(&base, f).ok_or_else(|| KernelError::Invalid("a picked edge can't be evaluated".into()))?;
            let tan = der.normalized().ok_or_else(|| KernelError::Invalid("a picked edge has a point with no direction".into()))?;
            let n = normal_at(face, forward, p)
                .ok_or_else(|| KernelError::Invalid("the face beside an edge has a point with no normal there: not supported yet".into()))?;
            let out = if face_left { tan.cross(n) } else { n.cross(tan) };
            let out = out.normalized().ok_or_else(|| KernelError::Invalid("the face beside an edge runs along it".into()))?;
            Ok((p, tan, n, field.ruling(tan, n, out)?))
        };
        let samples: Vec<(Vec3, Vec3, Vec3, Vec3)> = (0..SAMPLES).map(|k| at(k as f64 / (SAMPLES - 1) as f64)).collect::<Result<_>>()?;
        let (Some(first), Some(last)) = (samples.first().copied(), samples.last().copied()) else {
            return Err(KernelError::Failed("ruled surface: samples".into()));
        };
        let tops: Vec<Vec3> = samples.iter().map(|(p, _, _, r)| *p + *r * d).collect();
        // A plane through the edge and its rulings, when everything lies in one.
        let plane_normal = first.1.cross(first.3).normalized();
        let planar =
            plane_normal.is_some_and(|nn| samples.iter().map(|s| s.0).chain(tops.iter().copied()).all(|q| (q - first.0).dot(nn).abs() <= eps));
        let exact = affine(&samples, d, eps);
        let line = planar && base.degree == 1 && base.ctrl.len() == 2 && exact.is_some();
        let (base, top) = match exact {
            Some(m) => {
                let top = mapped(&base, |p| m.apply(p));
                (base, top)
            }
            None => fitted(&at, d)?,
        };
        let kind = if planar { Kind::Plane { o: first.0, t: first.1, r: first.3 } } else { Kind::Ruled };
        let tangent = |f: f64| point_tangent(&top, f).and_then(|(_, t)| t.normalized()).unwrap_or(first.1);
        let end = |(p, _, n, r): (Vec3, Vec3, Vec3, Vec3), tan: Vec3| End { at: p, ruling: r, top: p + r * d, tangent: tan, normal: n };
        Ok(Strip { start: end(first, tangent(0.0)), end: end(last, tangent(1.0)), base, top, kind, line, planar })
    }

    /// The strip's face on its own: the base edge, up the end side (`up_end`, points from the
    /// edge's end to `top_end`), back along the far edge, down the start side (`up_start`, from
    /// the edge's start to `top_start`).
    fn face(&self, up_start: &[Vec3], up_end: &[Vec3], top_start: Vec3, top_end: Vec3, tolerance: f64) -> Result<Brep> {
        let fail = || KernelError::Failed("ruled surface: a strip".into());
        let mut parts = Parts { tolerance, ..Parts::default() };
        let vertex = |p: Vec3, parts: &mut Parts| -> Result<u32> {
            let i = u32::try_from(parts.vertices.len()).map_err(|_| fail())?;
            parts.vertices.push(b::Vertex { point: [p.x, p.y, p.z], tolerance });
            Ok(i)
        };
        let [u0, u1] = range(&self.base);
        // The loop's corners and the curves between them, counter-clockwise about the strip's
        // natural normal (t × r).
        let (bs, be) = (point(&self.base, 0.0).ok_or_else(fail)?, point(&self.base, 1.0).ok_or_else(fail)?);
        let corners: Vec<u32> = [bs, be].into_iter().map(|p| vertex(p, &mut parts)).collect::<Result<_>>()?;
        let (base, base_iv) = spline_curve(&hpoints(&self.base), self.base.knots.clone())?;
        let mut uses: Vec<(b::Curve, [f64; 2], [u32; 2], bool, Option<Curve2Pair>)> = Vec::new();
        let ruled = matches!(self.kind, Kind::Ruled);
        uses.push((base, base_iv, [corners[0], corners[1]], true, ruled.then_some(([u0, 0.0], [u1, 0.0]))));
        let mut chain_up = |pts: &[Vec3], from: u32, u: f64, parts: &mut Parts, uses: &mut Vec<_>| -> Result<u32> {
            let mut at = from;
            for w in pts.windows(2) {
                let next = vertex(w[1], parts)?;
                let pc = ruled.then_some(([u, 0.0], [u, 1.0]));
                uses.push((b::Curve::Line { start: [w[0].x, w[0].y, w[0].z], end: [w[1].x, w[1].y, w[1].z] }, [0.0, 1.0], [at, next], true, pc));
                at = next;
            }
            Ok(at)
        };
        let end_top = chain_up(up_end, corners[1], u1, &mut parts, &mut uses)?;
        // Down the start side, made from the bottom up and walked backwards.
        let mut start_side = Vec::new();
        let start_top = chain_up(up_start, corners[0], u0, &mut parts, &mut start_side)?;
        let (top, top_iv) = if self.line {
            (b::Curve::Line { start: [top_start.x, top_start.y, top_start.z], end: [top_end.x, top_end.y, top_end.z] }, [0.0, 1.0])
        } else {
            spline_curve(&hpoints(&self.top), self.top.knots.clone())?
        };
        uses.push((top, top_iv, [start_top, end_top], false, ruled.then_some(([u0, 1.0], [u1, 1.0]))));
        for (curve, iv, ends, _, pc) in start_side.into_iter().rev() {
            uses.push((curve, iv, ends, false, pc));
        }
        let surface = match self.kind {
            Kind::Plane { o, t, r } => b::Surface::PlaneThrough {
                origin: [o.x, o.y, o.z],
                u_point: [o.x + t.x, o.y + t.y, o.z + t.z],
                v_point: [o.x + r.x, o.y + r.y, o.z + r.z],
            },
            Kind::Ruled => {
                let mut ctrl = hpoints(&self.base);
                ctrl.extend(hpoints(&self.top));
                // u along the edge (fastest), v across: the base row, then the top row.
                spline_surface(
                    &NurbsSurface::from_homogeneous([self.base.degree, 1], [self.base.knots.clone(), vec![0.0, 0.0, 1.0, 1.0]], ctrl)
                        .map_err(|e| KernelError::Failed(format!("ruled surface: {e}")))?,
                )
            }
        };
        face_of(parts, surface, uses)
    }
}

/// A pcurve given as a straight line between two (u, v) points over its edge's interval.
type Curve2Pair = ([f64; 2], [f64; 2]);

/// One face of the record `parts` (whose vertices are in place) on `surface`, bounded by one loop
/// of `uses`: each edge's curve, interval, vertices, whether the loop runs along it, and its
/// straight pcurve where known (else the shared pcurve routine's, exact for lines and splines on
/// a plane).
fn face_of(mut parts: Parts, surface: b::Surface, uses: Vec<(b::Curve, [f64; 2], [u32; 2], bool, Option<Curve2Pair>)>) -> Result<Brep> {
    let tolerance = parts.tolerance;
    let mut given = Vec::with_capacity(uses.len());
    for (curve, interval, vertices, forward, pc) in uses {
        parts.edges.push(b::Edge { curve, interval, vertices, tolerance, uses: Vec::new() });
        given.push((forward, pc.map(|(a, z)| uv_line(a, z, interval))));
    }
    let loop_uses = parts
        .edges
        .iter()
        .zip(&given)
        .enumerate()
        .map(|(i, (e, (forward, pc)))| {
            let edge = u32::try_from(i).map_err(|_| KernelError::Failed("ruled surface: an edge".into()))?;
            Ok(b::pcurve::EdgeUse { edge, curve: &e.curve, interval: e.interval, forward: *forward, tolerance, given: pc.as_ref() })
        })
        .collect::<Result<Vec<_>>>()?;
    let pcurves = b::pcurve::pcurves(&surface, &[loop_uses], tolerance).map_err(|e| KernelError::Failed(format!("ruled surface: {e}")))?;
    let edges = given
        .iter()
        .zip(pcurves.into_iter().flatten())
        .enumerate()
        .map(|(i, ((forward, _), pcurve))| {
            Ok(b::OrientedEdge {
                edge: u32::try_from(i).map_err(|_| KernelError::Failed("ruled surface: an edge".into()))?,
                forward: *forward,
                pcurve,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let loops = vec![b::Loop::Edges { edges }];
    let domain = match &surface {
        b::Surface::Spline { u_knots, v_knots, points } => [natural(u_knots, points.len()), natural(v_knots, points.first().map_or(0, Vec::len))],
        b::Surface::RationalSpline { u_knots, v_knots, homogeneous } => {
            [natural(u_knots, homogeneous.len()), natural(v_knots, homogeneous.first().map_or(0, Vec::len))]
        }
        _ => uv_bounds(&loops).ok_or_else(|| KernelError::Invalid("a ruled face encloses no area".into()))?,
    };
    parts.faces.push((b::Face { surface, domain, loops }, true));
    Ok(parts.canonical()?.brep)
}

/// A spline direction's knot domain.
fn natural(knots: &[f64], count: usize) -> [f64; 2] {
    let p = knots.len().saturating_sub(count + 1);
    [knots.get(p).copied().unwrap_or(0.0), knots.get(count).copied().unwrap_or(1.0)]
}

/// The box of a face's pcurves' control points.
fn uv_bounds(loops: &[b::Loop]) -> Option<[[f64; 2]; 2]> {
    let mut bx = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for l in loops {
        let b::Loop::Edges { edges } = l else { continue };
        for u in edges {
            let pts: Vec<[f64; 2]> = match &u.pcurve {
                b::Curve2::Spline { points, .. } => points.clone(),
                b::Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
            };
            for p in pts {
                for (d, x) in bx.iter_mut().zip(p) {
                    d[0] = d[0].min(x);
                    d[1] = d[1].max(x);
                }
            }
        }
    }
    bx.iter().all(|d| d[0].is_finite() && d[1] > d[0]).then_some(bx)
}

/// A flat polygon face through `pts` (three or more, in order), on the plane through its first
/// three points (its normal (b − a) × (c − a)).
fn polygon(pts: &[Vec3], tolerance: f64) -> Result<Brep> {
    let (Some(a), Some(bb), Some(c)) = (pts.first(), pts.get(1), pts.get(2)) else {
        return Err(KernelError::Failed("ruled surface: a corner".into()));
    };
    let mut parts = Parts { tolerance, ..Parts::default() };
    parts.vertices = pts.iter().map(|p| b::Vertex { point: [p.x, p.y, p.z], tolerance }).collect();
    let n = pts.len();
    let uses = (0..n)
        .map(|i| {
            let (p, q) = (pts[i], pts[(i + 1) % n]);
            let ends = [u32::try_from(i).unwrap_or(0), u32::try_from((i + 1) % n).unwrap_or(0)];
            (b::Curve::Line { start: [p.x, p.y, p.z], end: [q.x, q.y, q.z] }, [0.0, 1.0], ends, true, None)
        })
        .collect();
    let surface = b::Surface::PlaneThrough { origin: [a.x, a.y, a.z], u_point: [bb.x, bb.y, bb.z], v_point: [c.x, c.y, c.z] };
    face_of(parts, surface, uses)
}

/// An affine map taking each point of an edge to the end of its ruling.
enum Affine {
    Translate(Vec3),
    /// Points scaled by `scale` about the axis through `c` along `a` (in its square plane), then
    /// moved by `shift`.
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
    let centre = a + (v.cross(w) * u.dot(u) + w.cross(u) * v.dot(v)) * (1.0 / (2.0 * ww));
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

/// A ruled B-spline fitted to the strip (degree 1 across): cubics through matching samples of the
/// edge and of its rulings' ends (at the same parameters, so they share their knots), refined
/// until both are within [`FIT_TOLERANCE`]; the edge's fit and the far edge's.
fn fitted(at: &dyn Fn(f64) -> Result<(Vec3, Vec3, Vec3, Vec3)>, d: f64) -> Result<(CurveN<4>, CurveN<4>)> {
    let fail = |e: solidcraft_geom::bspline::SplineError| KernelError::Failed(format!("ruled surface: fitting a strip: {e}"));
    let lift = |c: CurveN<3>| CurveN { degree: c.degree, knots: c.knots, ctrl: c.ctrl.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect() };
    let mut m = SAMPLES + 1;
    loop {
        let params: Vec<f64> = (0..m).map(|k| k as f64 / (m - 1) as f64).collect();
        let pts: Vec<(Vec3, Vec3, Vec3, Vec3)> = params.iter().map(|f| at(*f)).collect::<Result<_>>()?;
        let near: Vec<[f64; 3]> = pts.iter().map(|s| [s.0.x, s.0.y, s.0.z]).collect();
        let far: Vec<[f64; 3]> = pts.iter().map(|s| s.0 + s.3 * d).map(|q| [q.x, q.y, q.z]).collect();
        let (base, top) = (lift(CurveN::interpolate(&params, &near, 3).map_err(fail)?), lift(CurveN::interpolate(&params, &far, 3).map_err(fail)?));
        // Checked between the samples, where a fit strays most.
        let mut worst = 0.0f64;
        for k in 0..m - 1 {
            let f = (k as f64 + 0.5) / (m - 1) as f64;
            let (p, _, _, r) = at(f)?;
            let (Some(q0), Some(q1)) = (point(&base, f), point(&top, f)) else {
                return Err(KernelError::Failed("ruled surface: fitting a strip".into()));
            };
            worst = worst.max(q0.dist(p)).max(q1.dist(p + r * d));
        }
        if worst <= FIT_TOLERANCE {
            return Ok((base, top));
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
    /// One side edge from the vertex up the ruling to `top`: the strips' rulings agree, or a free
    /// end.
    Shared { at: Vec3, top: Vec3 },
    /// Mitred: the edge from the vertex to where the strips' far edges meet (`m`); a curved strip
    /// keeps its own side edge and gets a triangle up to `m` (its own side's top given).
    Mitre { at: Vec3, m: Vec3, incoming: Option<Vec3>, outgoing: Option<Vec3> },
    /// Rounded: each strip's own side edge (to `incoming`, `outgoing`), and the sector SMLib
    /// revolves from one into the other about `axis` through the vertex by `turn`.
    Round { at: Vec3, incoming: Vec3, outgoing: Vec3, axis: Vec3, turn: f64 },
}

impl Joint {
    fn new(a: Option<&Strip>, b: Option<&Strip>, mode: &RuledMode, corners: RuledCorners, eps: f64) -> Result<Joint> {
        let (a, b) = match (a, b) {
            (Some(a), Some(b)) => (a, b),
            (Some(a), None) => return Ok(Joint::Shared { at: a.end.at, top: a.end.top }),
            (None, Some(b)) => return Ok(Joint::Shared { at: b.start.at, top: b.start.top }),
            (None, None) => return Err(KernelError::Failed("ruled surface: an empty joint".into())),
        };
        let (ea, eb) = (&a.end, &b.start);
        let vertex = ea.at;
        if (ea.ruling - eb.ruling).len() <= SAME_RULING {
            return Ok(Joint::Shared { at: vertex, top: (ea.top + eb.top) * 0.5 });
        }
        // Where the far edges, carried on straight, meet: λ along the incoming one past its end,
        // μ along the outgoing one past its start.
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
        let refuse = |what: &str| KernelError::Invalid(format!("{what} at the corner at {vertex:?}"));
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
            // A strip that isn't a plane keeps its side edge and gets a triangle up to the mitre.
            let own = |s: &Strip, e: &End| (!s.planar).then_some(e.top);
            return Ok(Joint::Mitre { at: vertex, m, incoming: own(a, ea), outgoing: own(b, eb) });
        }
        // Rounded: the incoming ruling turned into the outgoing one about an axis both keep the
        // same angle to (the reference direction, or the faces' normal: a cone), else about the
        // axis square to both (a flat sector).
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
        let (axis, turn) = if turn < 0.0 { (axis * -1.0, -turn) } else { (axis, turn) };
        if !(turn > 1e-9 && turn < std::f64::consts::PI - 1e-6) {
            return Err(refuse("the strips turn back on each other"));
        }
        Ok(Joint::Round { at: vertex, incoming: ea.top, outgoing: eb.top, axis, turn })
    }

    /// The points a strip's side runs through from the joint's vertex up (its edges are the lines
    /// between them), and the point its far edge starts or ends at.
    fn side(&self, which: Side, s: &Strip) -> (Vec<Vec3>, Vec3) {
        match *self {
            Joint::Shared { at, top } => (vec![at, top], top),
            Joint::Round { at, incoming, outgoing, .. } => {
                let top = if which == Side::In { incoming } else { outgoing };
                (vec![at, top], top)
            }
            Joint::Mitre { at, m, incoming, outgoing } => {
                if let Some(top) = if which == Side::In { incoming } else { outgoing } {
                    return (vec![at, top], top);
                }
                if s.line {
                    return (vec![at, m], m);
                }
                // A plane under a curved far edge: on along the mitre's far edge to it.
                let top = if which == Side::In { s.end.top } else { s.start.top };
                (vec![at, m, top], top)
            }
        }
    }

    /// The joint's own faces: a round corner's sector, a mitre's triangles.
    fn corner_faces(&self, tolerance: f64) -> Result<Vec<Brep>> {
        match *self {
            Joint::Shared { .. } => Ok(Vec::new()),
            Joint::Mitre { at, m, incoming, outgoing } => {
                let mut out = Vec::new();
                if let Some(top) = incoming {
                    out.push(polygon(&[at, m, top], tolerance)?);
                }
                if let Some(top) = outgoing {
                    out.push(polygon(&[at, top, m], tolerance)?);
                }
                Ok(out)
            }
            Joint::Round { at, incoming, axis, turn, .. } => {
                // The incoming side as a line in the plane it spans with the axis, turned about the
                // axis through the vertex (a cone with its apex there, or a flat sector).
                let side = incoming - at;
                let x = side.normalized().ok_or_else(|| KernelError::Failed("ruled surface: a round corner".into()))?;
                let y = (axis - x * axis.dot(x)).normalized().ok_or_else(|| KernelError::Failed("ruled surface: a round corner".into()))?;
                let plane = solidcraft_geom::Plane { origin: at, x, y };
                let line = solidcraft_geom::Seg2::Line { a: solidcraft_geom::Vec2::ZERO, b: solidcraft_geom::Vec2::new(side.len(), 0.0) };
                Ok(vec![crate::surfaces::revolved_record(&plane, &[line], at, axis, turn)?])
            }
        }
    }
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
    fn uses(b: &Body) -> HashMap<usize, usize> {
        b.record().edges.iter().enumerate().map(|(i, e)| (i, e.uses.len())).collect()
    }
    /// The edges of `s` that lie along a straight edge of `b` (their middles on it).
    fn on_edges_of(s: &Body, b: &Body) -> usize {
        let on = |p: Vec3| {
            b.record().edges.iter().any(|e| match e.curve {
                solidcraft_geom::brep::Curve::Line { start, end } => {
                    let (a, z) = (Vec3::new(start[0], start[1], start[2]), Vec3::new(end[0], end[1], end[2]));
                    p.dist_to_segment(a, z) < 1e-6
                }
                _ => false,
            })
        };
        s.record()
            .edges
            .iter()
            .filter(|e| {
                let c = solidcraft_geom::brep::eval::CurveEval::new(&e.curve).unwrap();
                on(c.point(0.5 * (e.interval[0] + e.interval[1])).unwrap())
            })
            .count()
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
            assert_eq!(on_edges_of(&s, &b), 4);
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
