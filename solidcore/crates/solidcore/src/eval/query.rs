//! Exact queries on a B-rep record: points and tangents along its edges, how far a point is from
//! an edge, where a point lies on its faces and which way they face there, and which faces a ray
//! meets. Every answer comes from the record's own curves, surfaces and pcurves in the kernel's
//! evaluation types ([`Curve`], [`Surface`], [`Curve2`]); nothing here meshes the body. Build a
//! [`Query`] once per record: it keeps each curve and surface ready to evaluate.
//!
//! The record reaches the kernel as plain data ([`Record`]): `solidcore-ops`' `query` copies an
//! application's record into it through `solidcore-convert`.
//!
//! Not upstream: SolidCraft's record queries (`crates/develop/src/query.rs`, from
//! `crates/geom/src/brep/query.rs`, MIT OR Apache-2.0), moved into the kernel with the sheet
//! development so that the application computes no geometry (docs/layers.md §1). A pcurve's or
//! edge's Bézier pieces come from the kernel's knot insertion (NLib's segment extraction and
//! Bézier decomposition) where SolidCraft used its own.
//!
//! What is exact and what is not: whether a point lies inside a face's loops is decided on its
//! pcurves' Bézier pieces exactly, or refused ([`crate::prism::profile::Loop2::holds`]); whether
//! it lies near an edge, on the edge's Bézier pieces by their hulls; a ray meets planes,
//! cylinders, cones and spheres in closed form and refuses the other surfaces. A point's
//! parameters on a surface are the evaluator's projection, and an edge's points
//! ([`Query::edge_points`]) are sampled to a sag.
//!
//! Deviations: none (no upstream counterpart). No recursion: [`ray_crossings`] and [`near`] halve
//! on explicit stacks, at most [`MAX_HALVINGS`] deep and [`MAX_PIECES`] in all.

// A comparison that is not true takes the refusing branch, so a NaN is refused too.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::OnceCell;

use super::vec::{v3, VecExt};
use super::{form, Curve, Curve2, CurveEval, Surface, SurfaceEval};
use crate::math::{Vec2, Vec3};
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::prism::profile::{Loop2, Seg2, MAX_BEZIER_CTRL, MAX_BEZIER_DEGREE};

/// Most pieces an edge is cut into by [`Query::edge_points`].
const MAX_EDGE_POINTS: usize = 4096;
/// Most halvings of one piece of an edge in [`Query::edge_points`].
const MAX_DEPTH: u32 = 16;
/// Newton steps of [`Query::edge_distance`].
const MAX_NEWTON: usize = 32;
/// Most halvings of one Bézier piece of an edge while [`near`] or [`ray_crossings`] decides: past
/// them a piece lies within rounding of its chord.
const MAX_HALVINGS: u32 = 52;
/// Most pieces [`near`] halves for one Bézier piece of an edge.
const MAX_PIECES: usize = 1 << 14;
/// The highest degree of a spline whose pieces are read.
const MAX_DEGREE: usize = super::spline::MAX_SPLINE_DEGREE;
/// The most knots of a spline whose pieces are read, its clamping included.
const MAX_KNOTS: usize = 1 << 14;
/// Furthest a control point may be from the origin along any axis (mm) for its spline's pieces
/// to be read.
const MAX_COORDINATE: f64 = 1e12;

/// A homogeneous control point `[x w, y w, z w, w]`.
type Hom = [f64; 4];

/// A curve's Bézier pieces, each its homogeneous control points.
type Pieces = Vec<Vec<Hom>>;

/// An edge of a [`Record`]: the curve its interval is on (`None` where it could not be read), its
/// interval, and the faces it bounds, once for each time a loop uses it.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    /// The curve the edge's interval is on: its NURBS form where it has one.
    pub curve: Option<Curve>,
    /// The edge's interval, from its start vertex to its end vertex.
    pub interval: [f64; 2],
    /// The faces it bounds, once per use (a periodic face's seam twice).
    pub faces: Vec<usize>,
}

/// One oriented edge of a face's loop: its edge, whether the loop runs along it, and its pcurve.
#[derive(Clone, Debug, PartialEq)]
pub struct Use {
    /// Into the record's edges.
    pub edge: usize,
    /// Whether the loop runs along the edge.
    pub forward: bool,
    /// The edge in the face's parameters, on the edge's interval.
    pub pcurve: Curve2,
}

/// A face's loop: a single vertex (its point, where the record has it), or edge uses.
#[derive(Clone, Debug, PartialEq)]
pub enum Loop {
    /// A vertex loop's point.
    Vertex(Option<[f64; 3]>),
    /// An edge loop's uses, in the order it runs.
    Edges(Vec<Use>),
}

/// A face of a [`Record`].
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// The surface the face's parameters are on: its NURBS form where it has one (`None` where it
    /// could not be read).
    pub parameters: Option<Surface>,
    /// What the face is, as its producer names it (`None` where the kernel has no surface of
    /// that kind): a ray meets this one.
    pub surface: Option<Surface>,
    /// The modelled rectangle in its parameters, `[[u0, u1], [v0, v1]]`.
    pub domain: [[f64; 2]; 2],
    /// Whether its shell uses it along its natural normal (`None` for a face no shell lists).
    pub forward: Option<bool>,
    /// Its loops.
    pub loops: Vec<Loop>,
}

/// A B-rep record as the queries read it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Record {
    /// The edges.
    pub edges: Vec<Edge>,
    /// The faces.
    pub faces: Vec<Face>,
}

/// A record ready for exact queries (see the module docs).
#[derive(Debug)]
pub struct Query {
    record: Record,
    curves: Vec<Option<CurveEval>>,
    surfaces: Vec<Option<SurfaceEval>>,
    /// Each face's edge loops in its own parameters, as the Bézier pieces of their pcurves
    /// ([`Query::trim_loops`]), read when first asked for.
    trims: Vec<OnceCell<Option<Vec<Pieces>>>>,
    /// Each edge's Bézier pieces in space ([`Query::edge_form`]), read when first asked for.
    forms: Vec<OnceCell<Option<Pieces>>>,
}

impl Query {
    /// The record made ready: each curve and surface set up to evaluate.
    #[must_use]
    pub fn new(record: Record) -> Query {
        let curves = record
            .edges
            .iter()
            .map(|e| e.curve.as_ref().and_then(|c| CurveEval::new(c).ok()))
            .collect();
        let surfaces = record
            .faces
            .iter()
            .map(|f| f.parameters.as_ref().and_then(|s| SurfaceEval::new(s).ok()))
            .collect();
        let trims = record.faces.iter().map(|_| OnceCell::new()).collect();
        let forms = record.edges.iter().map(|_| OnceCell::new()).collect();
        Query {
            record,
            curves,
            surfaces,
            trims,
            forms,
        }
    }

    /// The record it reads.
    #[must_use]
    pub fn record(&self) -> &Record {
        &self.record
    }

    /// Edge `e`'s point and derivative at `t` on its parameter. `None` for a traced
    /// intersection curve (its tangent is not held).
    #[must_use]
    pub fn edge_at(&self, e: usize, t: f64) -> Option<(Vec3, Vec3)> {
        self.curves.get(e)?.as_ref()?.derivs(t)
    }

    /// Edge `e`'s interval on its parameter, from its start vertex to its end vertex.
    #[must_use]
    pub fn edge_interval(&self, e: usize) -> Option<[f64; 2]> {
        Some(self.record.edges.get(e)?.interval)
    }

    /// The faces edge `e` bounds, once for each time a loop uses it.
    #[must_use]
    pub fn edge_faces(&self, e: usize) -> Vec<usize> {
        self.record
            .edges
            .get(e)
            .map_or_else(Vec::new, |edge| edge.faces.clone())
    }

    /// Points of edge `e` from its start to its end, each with its parameter: the ends, and
    /// between them as many as keep the curve within `sag` mm of the chord between two
    /// neighbours and turning less than a twentieth of a turn between them. A line is its two
    /// ends. A sample, not a bound.
    #[must_use]
    pub fn edge_points(&self, e: usize, sag: f64) -> Option<Vec<(f64, Vec3)>> {
        let curve = self.curves.get(e)?.as_ref()?;
        let [t0, t1] = self.edge_interval(e)?;
        let at = |t: f64| curve.point(t);
        let (p0, p1) = (at(t0)?, at(t1)?);
        if matches!(curve, CurveEval::Line { .. }) {
            return Some(vec![(t0, p0), (t1, p1)]);
        }
        let sag = sag.max(1e-12);
        let turn = (std::f64::consts::TAU / 20.0).cos();
        let mut out = vec![(t0, p0)];
        // Pieces still to look at; the last pushed is looked at first, so pushing the right half
        // before the left keeps the output in order.
        let mut todo = vec![(t0, p0, t1, p1, 0u32)];
        while let Some((a, pa, b, pb, depth)) = todo.pop() {
            let m = 0.5 * (a + b);
            let pm = at(m)?;
            let chord = pb - pa;
            let off = if chord.len() > 0.0 {
                (pm - pa).cross(chord).len() / chord.len()
            } else {
                pm.dist(pa)
            };
            let bent = match (curve.derivs(a), curve.derivs(b)) {
                (Some((_, da)), Some((_, db))) => da
                    .normalized()
                    .zip(db.normalized())
                    .is_none_or(|(x, y)| x.dot(y) < turn),
                _ => false,
            };
            if (off > sag || bent || (depth == 0 && pa.dist(pb) <= sag))
                && depth < MAX_DEPTH
                && out.len() + todo.len() < MAX_EDGE_POINTS
            {
                todo.push((m, pm, b, pb, depth + 1));
                todo.push((a, pa, m, pm, depth + 1));
            } else {
                out.push((b, pb));
            }
        }
        Some(out)
    }

    /// How far `p` is from edge `e`: from its nearest point, found from the nearest of its
    /// [`Query::edge_points`] and Newton's method on the curve itself.
    #[must_use]
    pub fn edge_distance(&self, e: usize, p: Vec3, sag: f64) -> Option<f64> {
        let points = self.edge_points(e, sag)?;
        let [t0, t1] = self.edge_interval(e)?;
        let (mut t, mut best) = points
            .iter()
            .map(|(t, q)| (*t, q.dist(p)))
            .min_by(|x, y| x.1.total_cmp(&y.1))?;
        for _ in 0..MAX_NEWTON {
            let Some((q, d)) = self.edge_at(e, t) else {
                break;
            };
            best = best.min(q.dist(p));
            let dd = d.dot(d);
            if !(dd > 1e-300) {
                break;
            }
            // Held to the interval (a step that is not a number stays one, and ends the walk).
            let next = t - (q - p).dot(d) / dd;
            let next = if next < t0 {
                t0
            } else if next > t1 {
                t1
            } else {
                next
            };
            if (next - t).abs() <= 1e-15 * (1.0 + t.abs()) {
                break;
            }
            t = next;
        }
        Some(best)
    }

    /// The parameters on face `f` of `p`, a point within `accept` mm of its surface, brought
    /// into the face's own domain where the surface turns round.
    #[must_use]
    pub fn face_uv(&self, f: usize, p: Vec3, accept: f64) -> Option<[f64; 2]> {
        let s = self.surfaces.get(f)?.as_ref()?;
        let face = self.record.faces.get(f)?;
        let mut uv = s.project(p, None, accept)?;
        // A plane's projection takes any point: this one must lie on the surface.
        if !s.point(uv).is_some_and(|q| q.dist(p) <= accept) {
            return None;
        }
        let periods = s.domain().period;
        for (k, slot) in uv.iter_mut().enumerate() {
            if let (Some(period), Some([lo, hi])) =
                (periods.get(k).copied().flatten(), face.domain.get(k))
            {
                let middle = 0.5 * (lo + hi);
                *slot += ((middle - *slot) / period).round() * period;
            }
        }
        Some(uv)
    }

    /// Face `f`'s unit normal at its parameters `uv`, out of the material: the natural normal,
    /// turned where the face's shell uses the face against it.
    #[must_use]
    pub fn face_normal(&self, f: usize, uv: [f64; 2]) -> Option<Vec3> {
        let (_, su, sv) = self.surfaces.get(f)?.as_ref()?.derivs(uv[0], uv[1])?;
        let n = su.cross(sv).normalized()?;
        Some(if self.record.faces.get(f)?.forward? {
            n
        } else {
            -n
        })
    }

    /// Whether `p` lies on face `f`: within `accept` mm of its surface, and inside its loops
    /// or within `accept` mm of one of them in space (a face holds its own edges).
    #[must_use]
    pub fn face_contains(&self, f: usize, p: Vec3, accept: f64) -> bool {
        self.face_uv(f, p, accept)
            .is_some_and(|uv| self.inside(f, uv) == Some(true) || self.on_loops(f, p, accept))
    }

    /// The faces `p` lies on (see [`Query::face_contains`]), nearest first, each with how far
    /// `p` is from it.
    #[must_use]
    pub fn faces_at(&self, p: Vec3, accept: f64) -> Vec<(f64, usize)> {
        let mut out: Vec<(f64, usize)> = (0..self.record.faces.len())
            .filter_map(|f| {
                let uv = self.face_uv(f, p, accept)?;
                let q = self.surfaces.get(f)?.as_ref()?.point(uv)?;
                (self.inside(f, uv) == Some(true) || self.on_loops(f, p, accept))
                    .then_some((q.dist(p), f))
            })
            .collect();
        out.sort_by(|x, y| x.0.total_cmp(&y.0));
        out
    }

    /// Where the ray from `o` along `d` meets the faces ahead of `o` (further than `accept`):
    /// each meeting's distance along `d` (a unit vector), face and point, nearest first. Faces on
    /// planes, cylinders, cones and spheres (a ball patch's sphere too) are met exactly. A face on
    /// a torus, a spline, a revolution or a blend is not met exactly here yet, and an answer
    /// without it could miss a meeting.
    ///
    /// # Errors
    /// The first face the ray cannot meet exactly.
    pub fn ray(&self, o: Vec3, d: Vec3, accept: f64) -> Result<Vec<(f64, usize, Vec3)>, usize> {
        let Some(d) = d.normalized() else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for (f, face) in self.record.faces.iter().enumerate() {
            let surface = face.surface.as_ref().ok_or(f)?;
            for s in ray_surface(surface, o, d).ok_or(f)? {
                let q = o + d * s;
                if s > accept && self.face_contains(f, q, accept) {
                    out.push((s, f, q));
                }
            }
        }
        out.sort_by(|x, y| x.0.total_cmp(&y.0));
        Ok(out)
    }

    /// Whether `uv` lies inside face `f`, decided on its pcurves themselves by the even–odd rule
    /// over all its edge loops. Each loop is the planar loop of its pcurves' Bézier pieces
    /// ([`planar`]), which the kernel says holds a point or not exactly, or refuses
    /// ([`Loop2::holds`]). `None` where it refuses (`uv` on a loop or within rounding of it) and
    /// where a pcurve can't be read so.
    fn inside(&self, f: usize, uv: [f64; 2]) -> Option<bool> {
        let loops = self
            .trims
            .get(f)?
            .get_or_init(|| self.trim_loops(f))
            .as_ref()?;
        let p = Vec2::new(uv[0], uv[1]);
        let mut odd = false;
        for ring in loops {
            odd ^= match ring
                .iter()
                .map(|c| planar(c, p))
                .collect::<Option<Vec<Seg2>>>()
            {
                Some(segs) => Loop2 { segs }.holds(p)?,
                // A piece of a degree no profile segment holds: the ring's crossings of a ray
                // from `p`, counted on its pieces themselves.
                None => {
                    ring.iter()
                        .map(|c| ray_crossings(c, p))
                        .sum::<Option<usize>>()?
                        % 2
                        == 1
                }
            };
        }
        Some(odd)
    }

    /// Whether `p` lies within `accept` mm of one of face `f`'s loops, measured in space: from
    /// the curve of one of its edges ([`near`]), or a vertex loop's point. An edge with no NURBS
    /// form (a traced intersection) is measured by [`Query::edge_distance`].
    fn on_loops(&self, f: usize, p: Vec3, accept: f64) -> bool {
        let Some(face) = self.record.faces.get(f) else {
            return false;
        };
        face.loops.iter().any(|l| match l {
            Loop::Edges(edges) => edges.iter().any(|oe| self.near_edge(oe.edge, p, accept)),
            Loop::Vertex(point) => point.is_some_and(|v| v3(v).dist(p) <= accept),
        })
    }

    /// Whether `p` lies within `accept` mm of edge `e`'s curve (see [`Query::on_loops`]).
    fn near_edge(&self, e: usize, p: Vec3, accept: f64) -> bool {
        match self
            .forms
            .get(e)
            .map(|cell| cell.get_or_init(|| self.edge_form(e)))
        {
            Some(Some(pieces)) => pieces.iter().any(|c| near(c, p, accept)),
            _ => self
                .edge_distance(e, p, accept)
                .is_some_and(|d| d <= accept),
        }
    }

    /// Face `f`'s edge loops in its parameters, each the Bézier pieces of its pcurves in the order
    /// the loop runs (homogeneous `(u·w, v·w, 0, w)`). `None` where a pcurve is not a valid
    /// B-spline.
    fn trim_loops(&self, f: usize) -> Option<Vec<Pieces>> {
        let face = self.record.faces.get(f)?;
        let mut out = Vec::new();
        for l in &face.loops {
            let Loop::Edges(edges) = l else { continue };
            let mut ring = Vec::new();
            for oe in edges {
                let (knots, ctrl): (&[f64], Vec<Hom>) = match &oe.pcurve {
                    Curve2::Spline { knots, points } => (
                        knots,
                        points.iter().map(|q| [q[0], q[1], 0.0, 1.0]).collect(),
                    ),
                    Curve2::RationalSpline { knots, homogeneous } => (
                        knots,
                        homogeneous
                            .iter()
                            .map(|h| [h[0], h[1], 0.0, h[2]])
                            .collect(),
                    ),
                };
                let mut pieces = bezier_pieces(knots, &ctrl, oe.pcurve.interval()?)?;
                if !oe.forward {
                    pieces.reverse();
                    for c in &mut pieces {
                        c.reverse();
                    }
                }
                ring.extend(pieces);
            }
            out.push(ring);
        }
        Some(out)
    }

    /// Edge `e`'s Bézier pieces in space over its interval, on its NURBS form: a line's own
    /// segment, a polyline's segments, a circle's or an ellipse's rational arcs
    /// ([`form::curve`]), a spline's own pieces. `None` for a traced intersection, which has no
    /// such form.
    fn edge_form(&self, e: usize) -> Option<Pieces> {
        let edge = self.record.edges.get(e)?;
        let [t0, t1] = edge.interval;
        let line = |a: Vec3, b: Vec3| vec![[a.x, a.y, a.z, 1.0], [b.x, b.y, b.z, 1.0]];
        match edge.curve.as_ref()? {
            Curve::Line { start, end } => {
                let (a, d) = (v3(*start), v3(*end) - v3(*start));
                Some(vec![line(a + d * t0, a + d * t1)])
            }
            Curve::Polyline { points } => {
                // Segment i spans [i, i + 1] of the parameter.
                let mut out = Vec::new();
                for (i, pair) in points.windows(2).enumerate() {
                    let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else {
                        continue;
                    };
                    let (a, d) = (v3(a), v3(b) - v3(a));
                    let (lo, hi) = ((t0 - i as f64).max(0.0), (t1 - i as f64).min(1.0));
                    if lo < hi {
                        out.push(line(a + d * lo, a + d * hi));
                    }
                }
                Some(out)
            }
            c @ (Curve::Circle { .. } | Curve::Ellipse { .. }) => {
                let (nurbs, interval) = form::curve(c, [t0, t1])?;
                spline_pieces(&nurbs, interval)
            }
            c @ (Curve::Spline { .. } | Curve::RationalSpline { .. }) => spline_pieces(c, [t0, t1]),
            Curve::SurfaceIntersection { .. } => None,
        }
    }
}

/// The Bézier pieces of spline `c` (a B-spline or NURBS curve) over `interval`.
fn spline_pieces(c: &Curve, interval: [f64; 2]) -> Option<Pieces> {
    match c {
        Curve::Spline { knots, points } => {
            let ctrl: Vec<Hom> = points.iter().map(|q| [q[0], q[1], q[2], 1.0]).collect();
            bezier_pieces(knots, &ctrl, interval)
        }
        Curve::RationalSpline { knots, homogeneous } => bezier_pieces(knots, homogeneous, interval),
        _ => None,
    }
}

/// Whether `knots` and the homogeneous control points `ctrl` make a B-spline whose pieces can be
/// read: a degree from 1 to [`MAX_DEGREE`], `n + degree + 1` knots for `n` points (at most
/// [`MAX_KNOTS`]), knots that never decrease and a domain of some length, no inner knot repeated
/// more than the degree, positive weights that can be divided by, and points no further than
/// [`MAX_COORDINATE`] from the origin. Its degree and domain.
fn spline_shape(knots: &[f64], ctrl: &[Hom]) -> Option<(usize, f64, f64)> {
    let n = ctrl.len();
    let p = knots.len().checked_sub(n.checked_add(1)?)?;
    if !(1..=MAX_DEGREE).contains(&p) || n <= p || knots.len() + 2 * p > MAX_KNOTS {
        return None;
    }
    if !knots.iter().all(|k| k.is_finite())
        || knots.iter().zip(knots.iter().skip(1)).any(|(a, b)| b < a)
    {
        return None;
    }
    let (lo, hi) = (*knots.get(p)?, *knots.get(n)?);
    if !(lo < hi) {
        return None;
    }
    for run in knots.get(p + 1..n)?.chunk_by(|a, b| a == b) {
        if let Some(&u) = run.first() {
            if run.len() > p && lo < u && u < hi {
                return None;
            }
        }
    }
    for h in ctrl {
        let w = h[3];
        if !(w.is_finite() && w >= f64::MIN_POSITIVE) {
            return None;
        }
        let q = [h[0] / w, h[1] / w, h[2] / w];
        if !q.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORDINATE) {
            return None;
        }
    }
    Some((p, lo, hi))
}

/// The Bézier pieces over `[a, b]` of the B-spline on `knots` with homogeneous control points
/// `ctrl`: the curve's segment over the part of `[a, b]` within its domain, by knot insertion,
/// decomposed into its Bézier pieces, one per span of some length, in order. `None` for a curve
/// that is not a valid B-spline ([`spline_shape`]).
fn bezier_pieces(knots: &[f64], ctrl: &[Hom], [a, b]: [f64; 2]) -> Option<Pieces> {
    let (p, lo, hi) = spline_shape(knots, ctrl)?;
    let (from, to) = (a.max(lo), b.min(hi));
    if !(from < to) {
        return Some(Vec::new());
    }
    let cpts: Vec<CPoint> = ctrl
        .iter()
        .map(|h| CPoint {
            x: h[0],
            y: h[1],
            z: h[2],
            w: h[3],
        })
        .collect();
    let curve = crate::nurbs::crv_basic::Curve::new(cpts, p, knots.to_vec()).ok()?;
    let clamped = |k: &[f64]| {
        let same = |run: &[f64], u: f64| run.iter().all(|x| *x == u);
        k.get(..=p).is_some_and(|r| same(r, lo)) && k.len() > p && {
            let start = k.len() - p - 1;
            k.get(start..).is_some_and(|r| same(r, hi))
        }
    };
    let segment = if from == lo && to == hi && clamped(knots) {
        curve
    } else {
        curve.extract_seg(from, to).ok()?
    };
    let pieces = segment.decompose_bez().ok()?;
    let hom = |c: &CPoint| {
        let z = if c.z == NOZ { 0.0 } else { c.z };
        if c.w == NOW {
            [c.x, c.y, z, 1.0]
        } else {
            [c.x, c.y, z, c.w]
        }
    };
    let out: Pieces = pieces
        .iter()
        .map(|piece| piece.cpts().iter().map(hom).collect())
        .collect();
    out.iter()
        .flatten()
        .all(|h| h.iter().all(|x| x.is_finite()) && h[3] > 0.0)
        .then_some(out)
}

/// A Bézier piece split at its middle (de Casteljau): its two halves.
fn halves(c: &[Hom]) -> (Vec<Hom>, Vec<Hom>) {
    let mut q = c.to_vec();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    while let (Some(&first), Some(&last)) = (q.first(), q.last()) {
        left.push(first);
        right.push(last);
        q = q
            .iter()
            .zip(q.iter().skip(1))
            .map(|(a, b)| {
                [
                    a[0] * 0.5 + b[0] * 0.5,
                    a[1] * 0.5 + b[1] * 0.5,
                    a[2] * 0.5 + b[2] * 0.5,
                    a[3] * 0.5 + b[3] * 0.5,
                ]
            })
            .collect();
    }
    right.reverse();
    (left, right)
}

/// The point of a homogeneous control point, where its weight can be divided by and the point
/// is finite.
fn point_of(h: Hom) -> Option<Vec3> {
    let p = Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
    (h[3].abs() > f64::MIN_POSITIVE && p.is_finite()).then_some(p)
}

/// The distance from `p` to the segment from `a` to `b`.
fn dist_to_segment(p: Vec3, a: Vec3, b: Vec3) -> f64 {
    let ab = b - a;
    let l2 = ab.len2();
    let t = if l2 > 0.0 {
        ((p - a).dot(ab) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.dist(a + ab * t)
}

/// A Bézier piece `c` of a pcurve (homogeneous, its weights above 0) as a segment of a planar
/// loop, to tell whether the loop holds `p`. A polynomial piece is itself: of degree 1 a line, 2
/// a conic of weight 1, 3 a cubic, up to [`MAX_BEZIER_DEGREE`] a Bézier. A rational line is the
/// line between its ends, and a rational quadratic its conic with its end weights made 1
/// (`w₁/√(w₀w₂)`: the same curve). A rational piece of a higher degree is taken as seen from
/// `p`: `p + w(t)(c(t) − p)`, the polynomial curve on the homogeneous coefficients, which lies
/// the same way from `p` as `c(t)` at every `t` since every weight is above 0. It crosses every
/// line through `p` where `c` does, on the same side of `p`, so the even–odd rule reads the same
/// of it. `None` past [`MAX_BEZIER_DEGREE`].
fn planar(c: &[Hom], p: Vec2) -> Option<Seg2> {
    let n = c
        .len()
        .checked_sub(1)
        .filter(|n| (1..=MAX_BEZIER_DEGREE).contains(n))?;
    let w0 = c.first()?[3];
    let rational = c.iter().any(|h| h[3] != w0);
    let pts: Vec<Vec2> = if rational && n > 2 {
        c.iter()
            .map(|h| p + Vec2::new(h[0] - p.x * h[3], h[1] - p.y * h[3]))
            .collect()
    } else {
        c.iter()
            .map(|h| Vec2::new(h[0] / h[3], h[1] / h[3]))
            .collect()
    };
    Some(match (pts.as_slice(), c) {
        ([a, b], _) => Seg2::Line { a: *a, b: *b },
        ([a, apex, b], [h0, h1, h2]) => Seg2::Conic {
            a: *a,
            apex: *apex,
            b: *b,
            w: if rational {
                h1[3] / (h0[3] * h2[3]).sqrt()
            } else {
                1.0
            },
        },
        ([p0, p1, p2, p3], _) => Seg2::Cubic {
            p0: *p0,
            p1: *p1,
            p2: *p2,
            p3: *p3,
        },
        _ => {
            let mut q = [Vec2::ZERO; MAX_BEZIER_CTRL];
            for (slot, x) in q.iter_mut().zip(&pts) {
                *slot = *x;
            }
            Seg2::Bezier {
                n: u8::try_from(n).ok()?,
                p: q,
            }
        }
    })
}

/// How many times the ray from `p` along +x crosses the Bézier piece `ctrl` (homogeneous, its
/// weights above 0), an end on the ray's line counting as above it, as [`Loop2::holds`] reads
/// ends. Decided on the piece itself: its height above the line times its weight is a polynomial
/// whose Bernstein coefficients are `w_i (y_i - p.y)`, so with one sign among them the piece keeps
/// to one side, and with one change of sign it crosses the line once (variation diminishing), on
/// the side of `p` its hull lies on when all of the hull lies on one side. Anything else is
/// halved, at most [`MAX_HALVINGS`] deep: `None` where that does not settle it (`p` on the piece,
/// to rounding).
fn ray_crossings(ctrl: &[Hom], p: Vec2) -> Option<usize> {
    let mut count = 0usize;
    let mut todo = vec![(ctrl.to_vec(), MAX_HALVINGS)];
    while let Some((c, depth)) = todo.pop() {
        if c.iter().any(|h| !(h[3] > 0.0)) {
            return None;
        }
        let above: Vec<bool> = c.iter().map(|h| h[1] - p.y * h[3] >= 0.0).collect();
        let changes = above
            .iter()
            .zip(above.iter().skip(1))
            .filter(|(a, b)| a != b)
            .count();
        if changes == 0 || c.iter().all(|h| h[0] < p.x * h[3]) {
            continue;
        }
        if changes == 1 && c.iter().all(|h| h[0] > p.x * h[3]) {
            count += 1;
            continue;
        }
        let depth = depth.checked_sub(1)?;
        let (a, b) = halves(&c);
        todo.push((b, depth));
        todo.push((a, depth));
    }
    Some(count)
}

/// Whether some point of the Bézier piece `ctrl` (homogeneous, its weights above 0) lies within
/// `reach` of `p`, decided on the piece itself. Its ends lie on it, and all of it within the hull
/// of its control points, so within the largest distance of a control point from its chord (the
/// segment between its ends) of that chord; a piece whose control points all lie on its chord (a
/// line) is its chord. A piece with an end within `reach` is near; one whose chord lies further
/// from `p` than `reach` and that distance is not; a line is near when its chord is; any other
/// is halved (de Casteljau) until one of those says. A piece halved [`MAX_HALVINGS`] times lies
/// within rounding of its chord, which is within `reach`, and so is near. Past [`MAX_PIECES`]
/// halvings in all a piece is near when its chord lies within `reach` and that distance.
fn near(ctrl: &[Hom], p: Vec3, reach: f64) -> bool {
    let mut todo = vec![(ctrl.to_vec(), 0u32)];
    let mut halved = 0usize;
    while let Some((c, depth)) = todo.pop() {
        let Some(pts) = c
            .iter()
            .map(|h| point_of(*h))
            .collect::<Option<Vec<Vec3>>>()
        else {
            continue;
        };
        let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else {
            continue;
        };
        if a.dist(p) <= reach || b.dist(p) <= reach {
            return true;
        }
        let stray = pts
            .iter()
            .map(|q| dist_to_segment(*q, a, b))
            .fold(0.0, f64::max);
        let gap = dist_to_segment(p, a, b);
        if stray == 0.0 && gap <= reach {
            return true;
        }
        if !(gap <= reach + stray) || stray == 0.0 {
            continue;
        }
        halved += 1;
        if depth >= MAX_HALVINGS || halved > MAX_PIECES {
            return true;
        }
        let (left, right) = halves(&c);
        todo.push((right, depth + 1));
        todo.push((left, depth + 1));
    }
    false
}

/// Where the line `o + s d` (unit `d`) meets `surface`: the values of `s`. Planes, cylinders,
/// cones and spheres, and a ball patch on its sphere; `None` for the other surfaces, which this
/// does not meet exactly yet.
fn ray_surface(surface: &Surface, o: Vec3, d: Vec3) -> Option<Vec<f64>> {
    let roots = |a: f64, b: f64, c: f64| -> Vec<f64> {
        if a.abs() <= 1e-300 {
            return if b.abs() > 1e-300 {
                vec![-c / b]
            } else {
                Vec::new()
            };
        }
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return Vec::new();
        }
        // The root that doesn't cancel first, the other from their product.
        let q = -0.5 * (b + b.signum() * disc.sqrt());
        let mut r = vec![q / a];
        if q.abs() > 1e-300 {
            r.push(c / q);
        }
        r
    };
    let plane = |origin: Vec3, n: Vec3| -> Vec<f64> {
        let den = d.dot(n);
        if den.abs() <= 1e-15 * n.len() {
            Vec::new()
        } else {
            vec![(origin - o).dot(n) / den]
        }
    };
    let sphere = |center: Vec3, radius: f64| -> Vec<f64> {
        let w = o - center;
        roots(1.0, 2.0 * w.dot(d), w.dot(w) - radius * radius)
    };
    Some(match surface {
        Surface::Plane {
            origin,
            u_axis,
            v_axis,
        } => plane(v3(*origin), v3(*u_axis).cross(v3(*v_axis))),
        Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => {
            let c = v3(*origin);
            plane(c, (v3(*u_point) - c).cross(v3(*v_point) - c))
        }
        Surface::Cylinder { frame, radius } => {
            let (c, z) = (v3(frame.origin), v3(frame.x_axis).cross(v3(frame.y_axis)));
            let (w, dd) = ((o - c) - z * (o - c).dot(z), d - z * d.dot(z));
            roots(dd.dot(dd), 2.0 * w.dot(dd), w.dot(w) - radius * radius)
        }
        Surface::Cone {
            frame,
            radius_at_origin,
            half_angle,
        } => {
            // r(v) = r0 + v tan(half), v along z; the radial distance squared is r(v)².
            let (c, z) = (v3(frame.origin), v3(frame.x_axis).cross(v3(frame.y_axis)));
            let k = half_angle.tan();
            let (w, dd) = ((o - c) - z * (o - c).dot(z), d - z * d.dot(z));
            let (h, dh) = ((o - c).dot(z), d.dot(z));
            let (r, dr) = (radius_at_origin + h * k, dh * k);
            roots(
                dd.dot(dd) - dr * dr,
                2.0 * (w.dot(dd) - r * dr),
                w.dot(w) - r * r,
            )
            .into_iter()
            .filter(|s| radius_at_origin + (h + dh * s) * k >= 0.0)
            .collect()
        }
        Surface::Sphere { frame, radius } => sphere(v3(frame.origin), *radius),
        Surface::BallPatch { center, radius } => sphere(v3(*center), *radius),
        Surface::Torus { .. }
        | Surface::Spline { .. }
        | Surface::RationalSpline { .. }
        | Surface::PlacedRevolution { .. } => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{ray_crossings, ray_surface, Hom};
    use crate::eval::{Frame, Surface};
    use crate::math::{Vec2, Vec3};

    /// A ball patch lies on its sphere, so a ray meets it where it meets the sphere: from
    /// (−5, 2, 3) along x, the sphere of radius 2 about (1, 2, 3) is met 6 ∓ 2 along, at 4 and 8
    /// (s² − 12 s + 32 = 0). A torus is not met exactly yet: no answer, rather than none met.
    #[test]
    fn a_ray_meets_a_ball_patch_on_its_sphere_and_refuses_a_torus() {
        let ball = Surface::BallPatch {
            center: [1., 2., 3.],
            radius: 2.,
        };
        let mut met = ray_surface(&ball, Vec3::new(-5., 2., 3.), Vec3::X).unwrap_or_default();
        met.sort_by(f64::total_cmp);
        assert_eq!(met, vec![4., 8.]);
        let frame = Frame {
            origin: [0.; 3],
            x_axis: [1., 0., 0.],
            y_axis: [0., 1., 0.],
        };
        let torus = Surface::Torus {
            frame,
            major_radius: 5.,
            minor_radius: 1.,
            tube_start: 0.,
        };
        assert_eq!(ray_surface(&torus, Vec3::new(-10., 0., 0.), Vec3::X), None);
    }

    /// A trim piece of a degree no profile segment holds (a line elevated to degree 14, as a fold's
    /// pcurves come) is read on the piece itself: the line from (0, 0) to (2, 2) crosses y = 1 at
    /// x = 1, so the ray along +x from (0.5, 1) crosses it once and the one from (1.5, 1) not at all;
    /// from (1, 1), on the piece, the halvings never settle. The same line with weights 1, 2, 1 ... (a
    /// rational piece) is the same line, read the same way.
    #[test]
    fn a_trim_piece_past_degree_nine_is_read_on_itself() {
        let line = |w: &dyn Fn(usize) -> f64| -> Vec<Hom> {
            (0..=14)
                .map(|i| {
                    let (s, w) = (i as f64 / 14.0, w(i));
                    [2.0 * s * w, 2.0 * s * w, 0.0, w]
                })
                .collect()
        };
        for ctrl in [
            line(&|_| 1.0),
            line(&|i| if i % 2 == 0 { 1.0 } else { 2.0 }),
        ] {
            assert_eq!(ray_crossings(&ctrl, Vec2::new(0.5, 1.0)), Some(1));
            assert_eq!(ray_crossings(&ctrl, Vec2::new(1.5, 1.0)), Some(0));
            assert_eq!(ray_crossings(&ctrl, Vec2::new(0.5, 3.0)), Some(0));
            assert_eq!(ray_crossings(&ctrl, Vec2::new(1.0, 1.0)), None);
        }
    }
}
