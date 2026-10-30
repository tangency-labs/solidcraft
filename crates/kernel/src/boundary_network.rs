//! Boundary surfaces (Surface › Boundary): one B-spline face through curves in one or two
//! directions (`solidcraft_geom::network`: a loft between curves, a Coons patch inside four, a
//! Gordon surface through a network), made from sketch curves, 3D sketch curves or a body's
//! edges. A boundary curve that is a chain of open edges of a surface may carry a continuity
//! condition with the face beside it: G1 (tangent) or G2 (curvature).
//!
//! The reusable pieces: [`CurveSource`] turns each kind of curve into a curve the network can
//! measure, [`EdgeFaces`] gives the frame and second fundamental form of the faces along a chain
//! of edges (what a G1 or G2 condition needs), and [`boundary_surface`] builds the face.
//!
//! The two-curve bridge with G0–G3 against a sketch's plane is [`crate::boundary`]; this is the
//! general case: three or four sides, interior curves, 3D curves and edges.

use solidcraft_geom::brep::Brep;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::bspline::{CurveN, NurbsSurface, SplineError};
use solidcraft_geom::network::{self, CrossFace, FaceFrame, NetCurve, NetworkCurve, PointCurve, Tolerances};
use solidcraft_geom::{Plane, Seg2, Vec3};

use crate::body::Body;
use crate::continuity_order::{Construction, Continuity};
use crate::surfaces::{Chain, Parts, spline_curve, spline_surface, surface_body, uv_line};
use crate::{KernelError, Result};

/// How far apart curves of the two directions may pass and still cross (mm).
pub const CROSS_TOLERANCE: f64 = 0.01;
/// How far the surface may stray from its curves (mm).
pub const FIT_TOLERANCE: f64 = 1e-4;
/// Most points of a 3D sketch curve a boundary surface follows (longer ones are resampled).
const MAX_WIRE_POINTS: usize = 400;
/// Most points picking the edges of one curve.
pub const MAX_EDGE_PICKS: usize = 10_000;
/// Most sketch curves one curve of a boundary surface chains.
pub const MAX_CHAIN_CURVES: usize = 1000;

/// The network's continuity for a curve's, refused past what a network builds
/// ([`Construction::Network`]).
fn net_order(c: &BoundaryCurve) -> Result<network::Continuity> {
    let named = |e: KernelError| match e {
        KernelError::Invalid(why) => KernelError::Invalid(format!("{}: {why}", c.label)),
        other => other,
    };
    match c.continuity.built_by(Construction::Network).map_err(named)? {
        Continuity::G0 => Ok(network::Continuity::G0),
        Continuity::G1 => Ok(network::Continuity::G1),
        Continuity::G2 => Ok(network::Continuity::G2),
        higher => Err(KernelError::Failed(format!("a curve network has no {} condition yet", higher.code()))),
    }
}

/// Where a curve of a boundary surface comes from.
pub enum CurveSource<'a> {
    /// Sketch segments on a plane, each starting where the one before it ends.
    Sketch { plane: Plane, segs: Vec<Seg2> },
    /// A 3D sketch curve's points: followed smoothly through them (a cubic spline).
    Points(Vec<Vec3>),
    /// A chain of a body's edges, each picked by a point on it.
    Edges { body: &'a Body, at: Vec<Vec3> },
    /// A side collapsed to a point: the apex of a three-sided patch, where the sides beside it
    /// meet.
    Point(Vec3),
}

/// A curve of a boundary surface: a name for messages, where it comes from, and (for edges of a
/// surface) its continuity with the face beside it.
pub struct BoundaryCurve<'a> {
    pub label: String,
    pub source: CurveSource<'a>,
    pub continuity: Continuity,
}

/// One B-spline face through curves `u` (two or more; or one with a continuity condition, crossed
/// by `v`) and `v` (none: a loft through `u`; two each way and no more: a Coons patch; more: a
/// Gordon surface through the network). A surface body; its history has no operands (the face
/// and its edges are new). Refused, naming the curves: curves that miss each other (by more than
/// [`CROSS_TOLERANCE`]) or cross twice, curves out of order, closed curves, a continuity
/// condition on anything but an open edge of a surface, or on curves of both directions.
pub fn boundary_surface(u: &[BoundaryCurve], v: &[BoundaryCurve]) -> Result<Body> {
    for c in u.iter().chain(v) {
        if let CurveSource::Edges { body, .. } = &c.source {
            body.require_brep("boundary surface")?;
        }
    }
    {
        let mut corners: Vec<Vec3> = Vec::new();
        let mut to_net = |c: &BoundaryCurve| -> Result<NetworkCurve<'static>> {
            let (curve, face): (Box<dyn NetCurve>, Option<EdgeFaces>) = match &c.source {
                CurveSource::Sketch { plane, segs } => (Box::new(SketchChain::new(&c.label, plane, segs)?), None),
                CurveSource::Points(pts) => (Box::new(Polyline::new(&c.label, pts)?), None),
                CurveSource::Point(p) => {
                    if !p.is_finite() {
                        return Err(KernelError::Invalid(format!("{} is not a finite point", c.label)));
                    }
                    (Box::new(PointCurve(*p)), None)
                }
                CurveSource::Edges { body, at } => {
                    if at.is_empty() || at.len() > MAX_EDGE_PICKS || at.iter().any(|p| !p.is_finite()) {
                        return Err(KernelError::Invalid(format!("{}: pick 1 to {MAX_EDGE_PICKS} points on its edges", c.label)));
                    }
                    let (chain, closed) = crate::surfaces::picked_chain(body, at)?;
                    if closed {
                        return Err(KernelError::Invalid(format!("not supported yet: {} is a closed chain of edges", c.label)));
                    }
                    // The chain's inner vertices: the face's sides are split there, so that a
                    // join shares the body's edges one for one.
                    let r = body.record();
                    for &(e, forward) in chain.iter().skip(1) {
                        let start = r.edges.get(e).and_then(|x| r.vertices.get(x.vertices[if forward { 0 } else { 1 }] as usize));
                        if let Some(v) = start {
                            corners.push(Vec3::new(v.point[0], v.point[1], v.point[2]));
                        }
                    }
                    let face = match c.continuity {
                        Continuity::G0 => None,
                        _ => Some(EdgeFaces::new(&c.label, body, &chain)?),
                    };
                    (Box::new(EdgeChain::new(&c.label, r, &chain)?), face)
                }
            };
            if c.continuity != Continuity::G0 && face.is_none() {
                return Err(KernelError::Invalid(format!("{}: G1 and G2 apply only to a body's edges", c.label)));
            }
            let order = net_order(c)?;
            Ok(NetworkCurve { label: c.label.clone(), curve, condition: face.map(|f| (order, Box::new(f) as Box<dyn CrossFace>)) })
        };
        let nu: Vec<NetworkCurve> = u.iter().map(&mut to_net).collect::<Result<_>>()?;
        let nv: Vec<NetworkCurve> = v.iter().map(&mut to_net).collect::<Result<_>>()?;
        let surface = network::network_surface(nu, nv, Tolerances { cross: CROSS_TOLERANCE, fit: FIT_TOLERANCE }).map_err(|e| match e {
            SplineError::Unsupported(m) => KernelError::Invalid(format!("not supported yet: {m}")),
            SplineError::TooLarge(m) => KernelError::TooLarge(m),
            SplineError::Invalid(m) => KernelError::Invalid(m),
        })?;
        face_of(&surface, &corners)
    }
}

/// `made` (a boundary surface) sewn into `target` along the edges they share: one body, a solid
/// when it closes the gap. Refused when they share no edge.
pub fn boundary_join(target: &Body, made: &Body) -> Result<Body> {
    target.require_brep("join")?;
    let joined = crate::surfaces::stitch(&[target, made], CROSS_TOLERANCE)?;
    if joined.edge_count() == target.edge_count() + made.edge_count() {
        return Err(KernelError::Invalid("the surface shares no edge with the body it joins".into()));
    }
    Ok(joined)
}

/// Sketch segments on a plane, run end to end by arc length.
struct SketchChain {
    plane: Plane,
    segs: Vec<Seg2>,
    /// Cumulative length at the start of each segment, and the total.
    starts: Vec<f64>,
    total: f64,
}

impl SketchChain {
    fn new(label: &str, plane: &Plane, segs: &[Seg2]) -> Result<SketchChain> {
        if segs.is_empty() || segs.len() > MAX_CHAIN_CURVES {
            return Err(KernelError::Invalid(format!("{label} has no curves, or too many")));
        }
        if segs.windows(2).any(|w| w[0].end().dist(w[1].start()) > 1e-6) {
            return Err(KernelError::Invalid(format!("{label}: its curves don't meet end to end")));
        }
        let mut starts = Vec::with_capacity(segs.len());
        let mut total = 0.0;
        for s in segs {
            starts.push(total);
            total += s.length();
        }
        if !(total > 0.0 && total.is_finite()) {
            return Err(KernelError::Invalid(format!("{label} has no length")));
        }
        Ok(SketchChain { plane: *plane, segs: segs.to_vec(), starts, total })
    }
}

impl NetCurve for SketchChain {
    fn point(&mut self, t: f64) -> Option<Vec3> {
        let at = t.clamp(0.0, 1.0) * self.total;
        let k = self.starts.partition_point(|s| *s <= at).saturating_sub(1);
        let (seg, start) = (self.segs.get(k)?, self.starts.get(k)?);
        let len = seg.length();
        let local = if len > 0.0 { ((at - start) / len).clamp(0.0, 1.0) } else { 0.0 };
        let p = self.plane.to_world(seg.point_at(local));
        p.is_finite().then_some(p)
    }
}

/// A 3D polyline followed smoothly: a cubic through its points by chord length (fewer points, a
/// lower degree; a long one resampled first).
struct Polyline {
    curve: CurveN<3>,
    end: f64,
}

impl Polyline {
    fn new(label: &str, pts: &[Vec3]) -> Result<Polyline> {
        let mut clean: Vec<Vec3> = Vec::with_capacity(pts.len());
        for p in pts {
            if !p.is_finite() {
                return Err(KernelError::Invalid(format!("{label} has a point that is not a finite number")));
            }
            if clean.last().is_none_or(|q: &Vec3| q.dist(*p) > 1e-9) {
                clean.push(*p);
            }
        }
        let pts = resample(&clean, MAX_WIRE_POINTS);
        if pts.len() < 2 {
            return Err(KernelError::Invalid(format!("{label} has no length")));
        }
        let mut params = vec![0.0];
        for w in pts.windows(2) {
            params.push(params.last().copied().unwrap_or(0.0) + w[0].dist(w[1]));
        }
        let end = params.last().copied().unwrap_or(0.0);
        let data: Vec<[f64; 3]> = pts.iter().map(|p| [p.x, p.y, p.z]).collect();
        let curve = CurveN::interpolate(&params, &data, (pts.len() - 1).min(3))
            .map_err(|e| KernelError::Invalid(format!("{label} can't be followed smoothly: {e}")))?;
        Ok(Polyline { curve, end })
    }
}

impl NetCurve for Polyline {
    fn point(&mut self, t: f64) -> Option<Vec3> {
        let ([x, y, z], _) = self.curve.eval(t.clamp(0.0, 1.0) * self.end)?;
        Some(Vec3::new(x, y, z))
    }
}

/// At most `max` points evenly along a polyline by arc length, its ends kept.
fn resample(pts: &[Vec3], max: usize) -> Vec<Vec3> {
    if pts.len() <= max || max < 2 {
        return pts.to_vec();
    }
    let mut cum = vec![0.0];
    for w in pts.windows(2) {
        cum.push(cum.last().copied().unwrap_or(0.0) + w[0].dist(w[1]));
    }
    let total = cum.last().copied().unwrap_or(0.0);
    (0..max)
        .filter_map(|k| {
            let at = total * k as f64 / (max - 1) as f64;
            let i = cum.partition_point(|c| *c <= at).saturating_sub(1).min(pts.len().saturating_sub(2));
            let (c0, c1) = (cum.get(i)?, cum.get(i + 1)?);
            let w = if c1 > c0 { ((at - c0) / (c1 - c0)).clamp(0.0, 1.0) } else { 0.0 };
            Some(pts.get(i)?.lerp(*pts.get(i + 1)?, w))
        })
        .collect()
}

/// A chain of a body's edges (each walked the way the chain runs along it), by their lengths;
/// a point a fraction of the way along an edge is at that fraction of its parameter interval.
struct EdgeChain {
    edges: Vec<(CurveEval, [f64; 2])>,
    starts: Vec<f64>,
    total: f64,
}

/// The curves of `chain` (edges of `r`), each with its interval in the chain's direction.
fn chain_curves(label: &str, r: &Brep, chain: &Chain) -> Result<Vec<(CurveEval, [f64; 2])>> {
    chain
        .iter()
        .map(|&(e, forward)| {
            let edge = r.edges.get(e).ok_or_else(|| KernelError::Invalid(format!("{label}: an edge reference")))?;
            let c = CurveEval::new(edge.parameters()).map_err(|err| KernelError::Invalid(format!("{label}: {err}")))?;
            let [t0, t1] = edge.interval;
            Ok((c, if forward { [t0, t1] } else { [t1, t0] }))
        })
        .collect()
}

/// The point `f` (0…1) of the way along a directed interval of a curve.
fn along((c, [a, z]): &(CurveEval, [f64; 2]), f: f64) -> Option<Vec3> {
    c.point(a + (z - a) * f.clamp(0.0, 1.0))
}

impl EdgeChain {
    fn new(label: &str, r: &Brep, chain: &Chain) -> Result<EdgeChain> {
        let edges = chain_curves(label, r, chain)?;
        let mut starts = Vec::with_capacity(edges.len());
        let mut total = 0.0;
        for e in &edges {
            let pts = (0..=16).map(|k| along(e, k as f64 / 16.0)).collect::<Option<Vec<_>>>();
            let pts = pts.ok_or_else(|| KernelError::Invalid(format!("{label}: an edge can't be evaluated")))?;
            starts.push(total);
            total += pts.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>();
        }
        if !(total > 0.0 && total.is_finite()) {
            return Err(KernelError::Invalid(format!("{label} has no length")));
        }
        Ok(EdgeChain { edges, starts, total })
    }
}

impl NetCurve for EdgeChain {
    fn point(&mut self, t: f64) -> Option<Vec3> {
        let at = t.clamp(0.0, 1.0) * self.total;
        let k = self.starts.partition_point(|s| *s <= at).saturating_sub(1);
        let start = *self.starts.get(k)?;
        let end = self.starts.get(k + 1).copied().unwrap_or(self.total);
        let local = if end > start { ((at - start) / (end - start)).clamp(0.0, 1.0) } else { 0.0 };
        along(self.edges.get(k)?, local)
    }
}

/// The faces along a chain of edges, each edge an open edge of one face of its body: their
/// frames and second fundamental forms where a boundary surface meets them (G1 and G2
/// conditions).
pub(crate) struct EdgeFaces {
    sides: Vec<EdgeSide>,
}

/// One edge of the chain: points along it (to find the edge nearest a point), the surface of
/// its face, whether the face's boundary runs along the chain (`along`), and the surface
/// parameters found last (the next search starts there).
struct EdgeSide {
    pts: Vec<Vec3>,
    surface: SurfaceEval,
    along: bool,
    hint: Option<[f64; 2]>,
}

impl EdgeFaces {
    pub(crate) fn new(label: &str, body: &Body, chain: &Chain) -> Result<EdgeFaces> {
        let r = body.record();
        let curves = chain_curves(label, r, chain)?;
        let mut sides = Vec::with_capacity(chain.len());
        for (&(e, forward), curve) in chain.iter().zip(&curves) {
            let edge = r.edges.get(e).ok_or_else(|| KernelError::Invalid(format!("{label}: an edge reference")))?;
            let [u] = edge.uses.as_slice() else {
                return Err(KernelError::Invalid(format!(
                    "{label}: G1 and G2 need an open edge of a surface (an edge with a face on one side only)"
                )));
            };
            // How the face's loop (counter-clockwise about its surface's natural normal) runs the edge.
            let face = r.faces.get(u.face as usize).ok_or_else(|| KernelError::Failed("boundary surface: an edge's face".into()))?;
            let in_loop = match face.loops.get(u.r#loop as usize) {
                Some(solidcraft_geom::brep::Loop::Edges { edges }) => edges.get(u.position as usize).map(|x| x.forward),
                _ => None,
            };
            let in_loop = in_loop.ok_or_else(|| KernelError::Failed("boundary surface: an edge's face".into()))?;
            let pts = (0..=16).map(|k| along(curve, k as f64 / 16.0)).collect::<Option<Vec<_>>>();
            let pts = pts.ok_or_else(|| KernelError::Invalid(format!("{label}: an edge can't be evaluated")))?;
            let surface = SurfaceEval::new(face.parameters()).map_err(|err| KernelError::Failed(format!("boundary surface: {err}")))?;
            sides.push(EdgeSide { pts, surface, along: in_loop == forward, hint: None });
        }
        Ok(EdgeFaces { sides })
    }
}

impl CrossFace for EdgeFaces {
    fn frame(&mut self, at: Vec3, tangent: Vec3) -> Option<FaceFrame> {
        // The edge nearest the point, and the chain's direction along it there.
        let near = |s: &EdgeSide| s.pts.windows(2).map(|w| at.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
        let k = (0..self.sides.len()).min_by(|a, b| near(&self.sides[*a]).total_cmp(&near(&self.sides[*b])))?;
        let side = self.sides.get_mut(k)?;
        let seg = side.pts.windows(2).min_by(|a, b| at.dist_to_segment(a[0], a[1]).total_cmp(&at.dist_to_segment(b[0], b[1])))?;
        let chain_dir = seg[1] - seg[0];
        let s = &side.surface;
        let [u, v] = s.project(at, side.hint, f64::INFINITY).or_else(|| s.project(at, None, f64::INFINITY))?;
        side.hint = Some([u, v]);
        let (_, su, sv) = s.derivs(u, v)?;
        let normal = su.cross(sv).normalized()?;
        // The face's loop runs counter-clockwise about its surface's normal, so the face lies to
        // the left of it: outward is the loop's direction × the normal.
        let wire = if side.along { chain_dir } else { -chain_dir };
        let outward = wire.cross(normal).normalized()?;
        // The second fundamental form, in the basis of the tangent asked about and outward.
        let t = (tangent - normal * tangent.dot(normal)).normalized()?;
        let b = (outward - t * outward.dot(t)).normalized()?;
        let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let det = e * g - f * f;
        if !(det.abs() > 1e-24) {
            return None;
        }
        // II(w, w) = L·α² + 2M·αβ + N·β² for w = α·S_u + β·S_v, with L, M, N the surface's
        // second derivatives along its unit normal (do Carmo, §3-3): exact, rational surfaces
        // included, and with nothing sampled beyond the edge, where the face's domain ends.
        let (suu, suv, svv) = s.second(u, v)?;
        let (l, m, nn) = (suu.dot(normal), suv.dot(normal), svv.dot(normal));
        let ii = |w: Vec3| -> Option<f64> {
            let (p, q) = (w.dot(su), w.dot(sv));
            let (alpha, beta) = ((p * g - q * f) / det, (q * e - p * f) / det);
            let out = l * alpha * alpha + 2.0 * m * alpha * beta + nn * beta * beta;
            out.is_finite().then_some(out)
        };
        let (tt, bb) = (ii(t)?, ii(b)?);
        let tb = (ii(t + b)? - tt - bb) / 2.0;
        let form = [tt, tb, bb];
        form.iter().all(|x| x.is_finite()).then_some(FaceFrame { normal, outward: b, form })
    }
}

/// The face of a network surface: its four sides from the surface's boundary rows, each split
/// at the `corners` that lie on it (inner vertices of edge chains), the pieces edges on the side's
/// own curve over parts of its interval. A side collapsed to a point (a three-sided patch) is left
/// out: the two sides beside it meet at one vertex there. Refused when more than one side
/// shrinks to a point.
fn face_of(s: &NurbsSurface, corners: &[Vec3]) -> Result<Body> {
    let fail = |m: &str| KernelError::Failed(format!("boundary surface: {m}"));
    let [nu, nv] = s.count();
    if nu < 2 || nv < 2 {
        return Err(fail("control net"));
    }
    let [ku, kv] = s.knots().clone();
    let [[u0, u1], [v0, v1]] = s.domain();
    let corner = |i: usize, j: usize| s.point(i, j).project().ok_or_else(|| fail("a corner has no position"));
    let c = [corner(0, 0)?, corner(nu - 1, 0)?, corner(nu - 1, nv - 1)?, corner(0, nv - 1)?];
    let size = c.iter().map(|p| p.dist(c[0])).fold(0.0, f64::max).max(1.0);
    let row = |j: usize| (0..nu).map(|i| s.point(i, j)).collect::<Vec<_>>();
    let column = |i: usize| (0..nv).map(|j| s.point(i, j)).collect::<Vec<_>>();
    // Bottom (v0) and top (v1) along u, left (u0) and right (u1) along v, each from its first
    // corner to its second; the loop runs bottom, right, top backwards, left backwards.
    let sides = [
        (row(0), ku.clone(), [0, 1], true, [None, Some(v0)]),
        (column(nu - 1), kv.clone(), [1, 2], true, [Some(u1), None]),
        (row(nv - 1), ku, [3, 2], false, [None, Some(v1)]),
        (column(0), kv, [0, 3], false, [Some(u0), None]),
    ];
    let collapsed: Vec<bool> = sides.iter().map(|(_, _, [a, z], ..)| c[*a].dist(c[*z]) <= size * 1e-9).collect();
    if collapsed.iter().filter(|x| **x).count() > 1 {
        return Err(KernelError::Invalid("not supported yet: a boundary surface with more than one side shrunk to a point".into()));
    }
    // Vertices: the corners, a collapsed side's two ends one vertex.
    let mut vertex_of = [0usize, 1, 2, 3];
    for (k, (_, _, [a, z], ..)) in sides.iter().enumerate() {
        if collapsed.get(k).copied().unwrap_or(false) {
            vertex_of[*z] = vertex_of[*a];
        }
    }
    let tolerance = (size * 1e-6).max(1e-5);
    let mut parts = Parts { tolerance, ..Parts::default() };
    parts.vertices = c.iter().map(|p| solidcraft_geom::brep::Vertex { point: [p.x, p.y, p.z], tolerance }).collect();
    let mut uses = Vec::new();
    // The same uses as plain (curve, interval, vertices, forward), for a flat patch.
    let mut flat_uses = Vec::new();
    for (k, (ctrl, knots, [a, z], forward, fixed)) in sides.into_iter().enumerate() {
        if collapsed.get(k).copied().unwrap_or(false) {
            continue;
        }
        let (curve, [t0, t1]) = spline_curve(&ctrl, knots)?;
        let eval = CurveEval::new(&curve).map_err(|e| fail(&e.to_string()))?;
        let cuts = cuts_on(&eval, [t0, t1], corners);
        let mut breaks = vec![t0];
        breaks.extend(cuts.iter().map(|(t, _)| *t));
        breaks.push(t1);
        let mut ends = vec![vertex_of[a]];
        for (_, p) in &cuts {
            ends.push(parts.vertices.len());
            parts.vertices.push(solidcraft_geom::brep::Vertex { point: [p.x, p.y, p.z], tolerance });
        }
        ends.push(vertex_of[z]);
        let uv = |t: f64| [fixed[0].unwrap_or(t), fixed[1].unwrap_or(t)];
        let mut pieces = Vec::with_capacity(breaks.len());
        let mut flat_side = Vec::with_capacity(breaks.len());
        for (w, e) in breaks.windows(2).zip(ends.windows(2)) {
            let interval = [w[0], w[1]];
            let edge = index(parts.edges.len())?;
            let vertices = [index(e[0])?, index(e[1])?];
            parts.edges.push(solidcraft_geom::brep::Edge { nurbs: None, curve: curve.clone(), interval, vertices, tolerance, uses: Vec::new() });
            flat_side.push((curve.clone(), interval, vertices, forward, None));
            pieces.push(solidcraft_geom::brep::OrientedEdge { edge, forward, pcurve: uv_line(uv(w[0]), uv(w[1]), interval) });
        }
        if !forward {
            pieces.reverse();
        }
        if !forward {
            flat_side.reverse();
        }
        flat_uses.extend(flat_side);
        uses.extend(pieces);
    }
    // A flat patch is its plane, not a spline lying in it: a spline face of straight-through
    // corners (an arc meeting a line at 180 degrees) is singular there, and an offset of it, a
    // thickening, has no normal to go along.
    let net: Vec<Vec3> = (0..nu).flat_map(|i| (0..nv).map(move |j| (i, j))).filter_map(|(i, j)| s.point(i, j).project()).collect();
    let rim: Vec<Vec3> =
        row(0).iter().chain(&column(nu - 1)).chain(row(nv - 1).iter().rev()).chain(column(0).iter().rev()).filter_map(|h| h.project()).collect();
    if let Some((origin, n)) = flat_net(&net, &rim, size * 1e-9) {
        let u = (c[1] - c[0]).cross(n).cross(n) * -1.0;
        let u = u.normalized().or_else(|| n.cross(Vec3::X).normalized()).or_else(|| n.cross(Vec3::Y).normalized());
        if let Some(u) = u {
            let a = |x: Vec3| [x.x, x.y, x.z];
            let surface = solidcraft_geom::brep::Surface::Plane { origin: a(origin), u_axis: a(u), v_axis: a(n.cross(u)) };
            parts.edges.clear();
            let brep = crate::surface_ruled::face_of(parts, surface, flat_uses)?;
            return crate::surfaces::all_new(surface_body(&brep)?, &[]);
        }
    }
    let face = solidcraft_geom::brep::Face {
        nurbs: None,
        surface: spline_surface(s),
        domain: s.domain(),
        loops: vec![solidcraft_geom::brep::Loop::Edges { edges: uses }],
    };
    parts.faces.push((face, true));
    let body = surface_body(&parts.canonical()?.brep)?;
    crate::surfaces::all_new(body, &[])
}

/// The plane the control points `net` all lie within `tol` of, as a point on it and its unit
/// normal turned the way the closed polygon `rim` runs round it, if there is one.
fn flat_net(net: &[Vec3], rim: &[Vec3], tol: f64) -> Option<(Vec3, Vec3)> {
    let origin = *rim.first()?;
    // Newell's normal of the rim: its length is twice the enclosed area.
    let mut n = Vec3::ZERO;
    for (p, q) in rim.iter().zip(rim.iter().cycle().skip(1)) {
        n += p.cross(*q);
    }
    let n = n.normalized()?;
    net.iter().all(|p| (*p - origin).dot(n).abs() <= tol).then_some((origin, n))
}
/// The points of `corners` on a side's curve (within [`CROSS_TOLERANCE`]) strictly between its
/// ends, with their parameters, in order: each found from its nearest of 256 samples by
/// golden-section search.
fn cuts_on(c: &CurveEval, [t0, t1]: [f64; 2], corners: &[Vec3]) -> Vec<(f64, Vec3)> {
    const SAMPLES: usize = 256;
    let margin = (t1 - t0) * 1e-6;
    let mut cuts: Vec<(f64, Vec3)> = corners
        .iter()
        .filter_map(|p| {
            let d = |t: f64| c.point(t).map_or(f64::INFINITY, |q| q.dist(*p));
            let at = |i: usize| t0 + (t1 - t0) * i as f64 / SAMPLES as f64;
            let i = (0..=SAMPLES).min_by(|a, b| d(at(*a)).total_cmp(&d(at(*b))))?;
            let (mut lo, mut hi) = (at(i.saturating_sub(1)), at((i + 1).min(SAMPLES)));
            let g = 0.5 * (5f64.sqrt() - 1.0);
            for _ in 0..80 {
                let (x, y) = (hi - g * (hi - lo), lo + g * (hi - lo));
                if d(x) < d(y) {
                    hi = y;
                } else {
                    lo = x;
                }
            }
            let t = 0.5 * (lo + hi);
            (t > t0 + margin && t < t1 - margin && d(t) <= CROSS_TOLERANCE).then(|| (t, c.point(t).unwrap_or(*p)))
        })
        .collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    cuts.dedup_by(|x, y| (x.0 - y.0).abs() <= margin);
    cuts
}

fn index(i: usize) -> Result<u32> {
    u32::try_from(i).map_err(|_| KernelError::TooLarge("boundary surface: too many edges".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch_region;
    use solidcraft_geom::{Loop2, Region2, Vec2};

    fn sketch(label: &str, plane: Plane, segs: Vec<Seg2>) -> BoundaryCurve<'static> {
        BoundaryCurve { label: label.into(), source: CurveSource::Sketch { plane, segs }, continuity: Continuity::G0 }
    }

    fn line3(label: &str, a: Vec3, b: Vec3) -> BoundaryCurve<'static> {
        BoundaryCurve { label: label.into(), source: CurveSource::Points(vec![a, b]), continuity: Continuity::G0 }
    }

    fn face_surface(b: &Body) -> SurfaceEval {
        SurfaceEval::new(&b.record().faces[0].surface).unwrap()
    }

    /// The flat sheet y ∈ [−10, 0], x ∈ [0, 10] in z = 0, its edge along y = 0 open.
    fn sheet() -> Body {
        let sq = Loop2::polygon(&[Vec2::new(0.0, -10.0), Vec2::new(10.0, -10.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 0.0)]);
        patch_region(&Plane::XY, &Region2 { outer: sq, holes: vec![] }).unwrap()
    }

    /// A Coons patch of four lines bounding the hyperbolic paraboloid z = x·y/10: every point of
    /// the face lies on it, and its area is the paraboloid's over the square.
    #[test]
    fn coons_patch_of_lines_is_a_hyperbolic_paraboloid() {
        let p = |x: f64, y: f64| Vec3::new(x, y, x * y / 10.0);
        let body = boundary_surface(
            &[line3("u0", p(0.0, 0.0), p(10.0, 0.0)), line3("u1", p(0.0, 10.0), p(10.0, 10.0))],
            &[line3("v0", p(0.0, 0.0), p(0.0, 10.0)), line3("v1", p(10.0, 0.0), p(10.0, 10.0))],
        )
        .unwrap();
        assert!(body.is_surface() && body.face_count() == 1);
        let s = face_surface(&body);
        for i in 0..=10 {
            for j in 0..=10 {
                let q = s.point([i as f64 / 10.0, j as f64 / 10.0]).unwrap();
                assert!((q.z - q.x * q.y / 10.0).abs() < 1e-9, "{q:?}");
            }
        }
    }

    /// Sketch curves: a patch inside two arcs and two lines on one plane lies in that plane.
    #[test]
    fn coplanar_sketch_curves_give_a_plane() {
        use std::f64::consts::PI;
        // Two half-discs' arcs over [0, 10] (radius 5) facing out, joined by the lines x = 0, 10.
        let low = Seg2::Arc { center: Vec2::new(5.0, 0.0), radius: 5.0, start: PI, sweep: PI };
        let high = Seg2::Arc { center: Vec2::new(5.0, 8.0), radius: 5.0, start: PI, sweep: -PI };
        let left = Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(0.0, 8.0) };
        let right = Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, 8.0) };
        let pl = Plane::XY.offset(3.0);
        let body = boundary_surface(
            &[sketch("low", pl, vec![low]), sketch("high", pl, vec![high])],
            &[sketch("left", pl, vec![left]), sketch("right", pl, vec![right])],
        )
        .unwrap();
        let s = face_surface(&body);
        for i in 0..=8 {
            for j in 0..=8 {
                assert!((s.point([i as f64 / 8.0, j as f64 / 8.0]).unwrap().z - 3.0).abs() < 1e-9);
            }
        }
    }

    /// A stadium: two half-circle arcs (radius 5) bulging out of the 10 × 8 rectangle between two
    /// lines. Each arc meets its lines at 180°, so the patch is singular at its corners, and the
    /// reference mesher's walk round the boundary used to run from such a corner onto the
    /// surface's extrapolation past its domain (a planar spline passes through every point of its
    /// plane there): the face measured 289 mm² for its 158.5. The vendored mesher's bounded
    /// projection (`nearest_projection`, vendor/README.md) keeps the walk on the domain. Its area
    /// is the stadium's, 80 + 25π, by the faces, the measuring mesh and the display mesh.
    #[test]
    fn a_patch_with_straight_corners_measures_its_own_area() {
        use std::f64::consts::PI;
        let low = Seg2::Arc { center: Vec2::new(5.0, 0.0), radius: 5.0, start: PI, sweep: PI };
        let high = Seg2::Arc { center: Vec2::new(5.0, 8.0), radius: 5.0, start: PI, sweep: -PI };
        let left = Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(0.0, 8.0) };
        let right = Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, 8.0) };
        let pl = Plane::XY.offset(3.0);
        let body = boundary_surface(
            &[sketch("low", pl, vec![low]), sketch("high", pl, vec![high])],
            &[sketch("left", pl, vec![left]), sketch("right", pl, vec![right])],
        )
        .unwrap();
        let want = 80.0 + 25.0 * PI;
        // Chords of the arcs at 0.01 mm sag cut off up to (2/3)·sag·length ≈ 0.21 mm².
        let near = |a: f64| (a - want).abs() < 0.25;
        let faces = body.faces(0.01).unwrap();
        assert!(faces.len() == 1 && near(faces[0].area), "faces: {faces:?}");
        let tess = body.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().measure().area;
        assert!(near(tess), "measuring mesh: {tess}");
        let display = body.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().measure().area;
        assert!(near(display), "display mesh: {display}");
    }

    /// Three sides: a quarter disc of radius 10 (two radii and the arc) is one face with three
    /// edges, the collapsed side left out of its loop, and measures 25π by its faces and both
    /// meshes.
    #[test]
    fn a_three_sided_patch_is_one_face_with_three_edges() {
        use std::f64::consts::{FRAC_PI_2, PI};
        let r = 10.0;
        let arc = Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: r, start: 0.0, sweep: FRAC_PI_2 };
        let x = Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(r, 0.0) };
        let y = Seg2::Line { a: Vec2::new(0.0, r), b: Vec2::new(0.0, 0.0) };
        let pl = Plane::XY;
        let apex = BoundaryCurve { label: "centre".into(), source: CurveSource::Point(Vec3::ZERO), continuity: Continuity::G0 };
        let body = boundary_surface(&[sketch("arc", pl, vec![arc]), apex], &[sketch("x", pl, vec![x]), sketch("y", pl, vec![y])]).unwrap();
        assert!(body.is_surface() && body.face_count() == 1);
        assert_eq!(body.edge_count(), 3, "the collapsed side is no edge");
        let want = PI * r * r / 4.0;
        let near = |a: f64| (a - want).abs() < 0.2;
        let faces = body.faces(0.01).unwrap();
        assert!(near(faces[0].area), "faces: {faces:?}");
        let tess = body.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().measure().area;
        assert!(near(tess), "measuring mesh: {tess}");
        let display = body.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().measure().area;
        assert!(near(display), "display mesh: {display}");
        // Two collapsed sides are no patch.
        let p = |l: &str, q: Vec3| BoundaryCurve { label: l.into(), source: CurveSource::Point(q), continuity: Continuity::G0 };
        assert!(
            boundary_surface(&[sketch("arc", pl, vec![arc]), p("o", Vec3::ZERO)], &[p("a", Vec3::new(r, 0.0, 0.0)), sketch("y", pl, vec![y])])
                .is_err()
        );
        assert!(boundary_surface(&[sketch("arc", pl, vec![arc]), p("nan", Vec3::new(f64::NAN, 0.0, 0.0))], &[]).is_err());
    }

    /// The angle between the two faces' normals across the edge at `p`.
    fn normal_angle(a: &SurfaceEval, b: &SurfaceEval, p: Vec3) -> f64 {
        let n = |s: &SurfaceEval| {
            let [u, v] = s.project(p, None, f64::INFINITY).unwrap();
            let (_, su, sv) = s.derivs(u, v).unwrap();
            su.cross(sv).normalized().unwrap()
        };
        n(a).dot(n(b)).abs().clamp(0.0, 1.0).acos().to_degrees()
    }

    /// G1 against a flat sheet's open edge: a loft from the edge to a line above, and a patch
    /// whose sides are arcs leaving the sheet tangent to it. The normals across the shared edge
    /// agree within 0.1°; the patch joins the sheet into one surface body.
    #[test]
    fn g1_meets_the_sheet_tangent() {
        let base = sheet();
        let edge =
            |g| BoundaryCurve { label: "edge".into(), source: CurveSource::Edges { body: &base, at: vec![Vec3::new(5.0, 0.0, 0.0)] }, continuity: g };
        let far = || line3("far", Vec3::new(0.0, 10.0, 5.0), Vec3::new(10.0, 10.0, 5.0));
        let loft = boundary_surface(&[edge(Continuity::G1), far()], &[]).unwrap();
        let plain = boundary_surface(&[edge(Continuity::G0), far()], &[]).unwrap();
        let (sb, sl, sp) = (face_surface(&base), face_surface(&loft), face_surface(&plain));
        for k in 0..=10 {
            let p = Vec3::new(k as f64, 0.0, 0.0);
            let a = normal_angle(&sb, &sl, p);
            assert!(a < 0.1, "G1 at x = {k}: {a}°");
        }
        assert!(normal_angle(&sb, &sp, Vec3::new(5.0, 0.0, 0.0)) > 10.0, "G0 has a crease");
        // Arcs in x = 0 and x = 10 from the edge, tangent to the sheet, up to the far line.
        let arc_plane = |x: f64| Plane::new(Vec3::new(x, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        let arc = Seg2::Arc { center: Vec2::new(0.0, 12.5), radius: 12.5, start: -std::f64::consts::FRAC_PI_2, sweep: 0.927_295_218_001_612_2 };
        let patch = boundary_surface(
            &[edge(Continuity::G1), far()],
            &[sketch("left", arc_plane(0.0), vec![arc]), sketch("right", arc_plane(10.0), vec![arc])],
        )
        .unwrap();
        let spa = face_surface(&patch);
        for k in 0..=10 {
            let a = normal_angle(&sb, &spa, Vec3::new(k as f64, 0.0, 0.0));
            assert!(a < 0.1, "G1 patch at x = {k}: {a}°");
        }
        let joined = boundary_join(&base, &patch).unwrap();
        assert_eq!(joined.face_count(), 2);
        assert!(
            boundary_join(&base, &boundary_surface(&[far(), line3("other", Vec3::new(0.0, 20.0, 5.0), Vec3::new(10.0, 20.0, 5.0))], &[]).unwrap())
                .is_err()
        );
    }

    /// Normal curvature of `s` at the point nearest `p` in the direction `w` (projected into the
    /// tangent plane), signed against the unit normal `n`.
    fn normal_curvature(s: &SurfaceEval, p: Vec3, w: Vec3, n: Vec3) -> f64 {
        let [u, v] = s.project(p, None, f64::INFINITY).unwrap();
        let (_, su, sv) = s.derivs(u, v).unwrap();
        let (suu, suv, svv) = s.second(u, v).unwrap();
        let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let (l, m, nn) = (suu.dot(n), suv.dot(n), svv.dot(n));
        let (p1, q1) = (w.dot(su), w.dot(sv));
        let det = e * g - f * f;
        let (a, b) = ((p1 * g - q1 * f) / det, (q1 * e - p1 * f) / det);
        (l * a * a + 2.0 * m * a * b + nn * b * b) / (e * a * a + 2.0 * f * a * b + g * b * b)
    }

    /// G2 against a real cylinder's open edge (R = 10, the face below the edge): across the edge
    /// the surface's normal curvature is the cylinder's, −1/R about its outward normal, within
    /// 0.1 % — also when the far curve is sheared along the edge, so that the surface's own
    /// cross direction leans. With G1 alone it is not.
    #[test]
    fn g2_matches_a_cylinder_across_its_edge() {
        let r = 10.0;
        let pl = Plane::new(Vec3::ZERO, Vec3::Y, Vec3::Z).unwrap();
        let arc = Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: r, start: -std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::FRAC_PI_2 };
        let cyl = crate::extrude_sheet(&pl, &[vec![arc]], 0.0, 10.0).unwrap();
        let sc = face_surface(&cyl);
        let made = |g, shear: f64| {
            let edge =
                BoundaryCurve { label: "rim".into(), source: CurveSource::Edges { body: &cyl, at: vec![Vec3::new(5.0, r, 0.0)] }, continuity: g };
            let top = line3("top", Vec3::new(shear, 3.0, 12.0), Vec3::new(10.0 + shear, 3.0, 12.0));
            face_surface(&boundary_surface(&[edge, top], &[]).unwrap())
        };
        for shear in [0.0, 5.0] {
            let sm = made(Continuity::G2, shear);
            for k in 1..=9 {
                let p = Vec3::new(k as f64, r, 0.0);
                let kc = normal_curvature(&sc, p, Vec3::Z, Vec3::Y);
                assert!((kc + 1.0 / r).abs() < 1e-9, "the cylinder's own curvature: {kc}");
                let km = normal_curvature(&sm, p, Vec3::Z, Vec3::Y);
                assert!((km - kc).abs() < 1e-3 / r, "shear {shear}, x = {k}: {km} across the edge where the cylinder has {kc}");
                // Along the edge both are straight.
                assert!(normal_curvature(&sm, p, Vec3::X, Vec3::Y).abs() < 1e-6);
                assert!(normal_angle(&sc, &sm, p) < 0.1);
            }
        }
        let g1 = made(Continuity::G1, 0.0);
        let k1 = normal_curvature(&g1, Vec3::new(5.0, r, 0.0), Vec3::Z, Vec3::Y);
        assert!((k1 + 1.0 / r).abs() > 0.1 / r, "G1 alone does not carry the curvature: {k1}");
    }

    /// G3 and G4 are more than a network holds: refused, naming the curve, never a panic.
    #[test]
    fn g3_is_not_supported() {
        let base = sheet();
        for g in [Continuity::G3, Continuity::G4] {
            let edge =
                BoundaryCurve { label: "edge".into(), source: CurveSource::Edges { body: &base, at: vec![Vec3::new(5.0, 0.0, 0.0)] }, continuity: g };
            let e = boundary_surface(&[edge, line3("far", Vec3::new(0.0, 10.0, 5.0), Vec3::new(10.0, 10.0, 5.0))], &[]).unwrap_err();
            assert!(e.to_string().contains("not supported yet") && e.to_string().contains("edge") && e.to_string().contains(g.as_str()), "{e}");
        }
    }

    /// Refusals: G1 on a sketch curve, on an edge between two faces, curves that miss.
    #[test]
    fn bad_boundaries_are_refused() {
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let edge = BoundaryCurve {
            label: "edge".into(),
            source: CurveSource::Edges { body: &cube, at: vec![Vec3::new(5.0, 0.0, 0.0)] },
            continuity: Continuity::G1,
        };
        let far = line3("far", Vec3::new(0.0, 10.0, 15.0), Vec3::new(10.0, 10.0, 15.0));
        let e = boundary_surface(&[edge, far], &[]).unwrap_err();
        assert!(e.to_string().contains("open edge"), "{e}");
        let mut tangent = line3("t", Vec3::ZERO, Vec3::X);
        tangent.continuity = Continuity::G1;
        assert!(boundary_surface(&[tangent, line3("b", Vec3::Y, Vec3::new(1.0, 1.0, 0.0))], &[]).is_err());
        let miss = boundary_surface(
            &[line3("a", Vec3::ZERO, Vec3::X * 10.0), line3("b", Vec3::Y * 10.0, Vec3::new(10.0, 10.0, 0.0))],
            &[line3("c", Vec3::ZERO, Vec3::Y * 10.0), line3("d", Vec3::new(10.0, 0.0, 2.0), Vec3::new(10.0, 10.0, 2.0))],
        )
        .unwrap_err();
        assert!(miss.to_string().contains("don't meet"), "{miss}");
        let pts = BoundaryCurve { label: "p".into(), source: CurveSource::Points(vec![Vec3::ZERO; 3]), continuity: Continuity::G0 };
        assert!(boundary_surface(&[pts, line3("b", Vec3::Y, Vec3::new(1.0, 1.0, 0.0))], &[]).is_err());
        let nan = BoundaryCurve {
            label: "n".into(),
            source: CurveSource::Points(vec![Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 0.0)]),
            continuity: Continuity::G0,
        };
        assert!(boundary_surface(&[nan, line3("b", Vec3::Y, Vec3::new(1.0, 1.0, 0.0))], &[]).is_err());
    }
}
