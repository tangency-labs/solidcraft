//! Exact queries on a record: points and tangents along its edges, how far a point is from an
//! edge, where a point lies on its faces and which way they face there, and which faces a ray
//! meets. Every answer comes from the record's own curves, surfaces and pcurves; nothing here
//! meshes the body. Build a [`Query`] once per record: it keeps each curve and surface ready to
//! evaluate.

use solidcraft_geom::prelude::*;
use std::cell::OnceCell;

use solidcore_ops::prism::loop_holds;
use solidcore_record::brep::{Brep, Curve, Curve2, Loop, Shell, Surface};
use solidcore_record::{Loop2, MAX_BEZIER_CTRL, MAX_BEZIER_DEGREE, Seg2, Vec2, Vec3};
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::brep::form;
use solidcraft_geom::bspline::{HPoint, NurbsCurve, bezier_segment};

/// Most pieces an edge is cut into by [`Query::edge_points`].
const MAX_EDGE_POINTS: usize = 4096;
/// Most halvings of one piece of an edge in [`Query::edge_points`].
const MAX_DEPTH: u32 = 16;
/// Newton steps of [`Query::edge_distance`].
const MAX_NEWTON: usize = 32;
/// Most halvings of one Bézier piece of an edge while [`near`] decides: past them a piece lies
/// within rounding of its chord.
const MAX_HALVINGS: u32 = 52;
/// Most pieces [`near`] halves for one Bézier piece of an edge.
const MAX_PIECES: usize = 1 << 14;

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// A curve's Bézier pieces, each its homogeneous control points.
type Pieces = Vec<Vec<HPoint>>;

/// A record ready for exact queries (see the module docs).
pub struct Query<'a> {
    record: &'a Brep,
    curves: Vec<Option<CurveEval>>,
    surfaces: Vec<Option<SurfaceEval>>,
    /// Whether each face's shell uses it along its natural normal (`None` for a face no shell
    /// lists, which a validated record has none of).
    forward: Vec<Option<bool>>,
    /// Each face's edge loops in its own parameters, as the Bézier pieces of their pcurves
    /// ([`Query::trim_loops`]), read when first asked for.
    trims: Vec<OnceCell<Option<Vec<Pieces>>>>,
    /// Each edge's Bézier pieces in space ([`Query::edge_form`]), read when first asked for.
    forms: Vec<OnceCell<Option<Pieces>>>,
}

impl<'a> Query<'a> {
    pub fn new(record: &'a Brep) -> Query<'a> {
        let mut forward = vec![None; record.faces.len()];
        for shell in &record.shells {
            if let Shell::Faces { faces, .. } = shell {
                for of in faces {
                    if let Some(slot) = forward.get_mut(of.face as usize) {
                        *slot = Some(of.forward);
                    }
                }
            }
        }
        Query {
            record,
            curves: record.edges.iter().map(|e| CurveEval::new(e.parameters()).ok()).collect(),
            surfaces: record.faces.iter().map(|f| SurfaceEval::new(f.parameters()).ok()).collect(),
            forward,
            trims: record.faces.iter().map(|_| OnceCell::new()).collect(),
            forms: record.edges.iter().map(|_| OnceCell::new()).collect(),
        }
    }

    pub fn record(&self) -> &'a Brep {
        self.record
    }

    /// Edge `e`'s point and derivative at `t` on its parameter. `None` for a traced
    /// intersection curve (its tangent is not held).
    pub fn edge_at(&self, e: usize, t: f64) -> Option<(Vec3, Vec3)> {
        self.curves.get(e)?.as_ref()?.derivs(t)
    }

    /// Edge `e`'s interval on its parameter, from its start vertex to its end vertex.
    pub fn edge_interval(&self, e: usize) -> Option<[f64; 2]> {
        Some(self.record.edges.get(e)?.interval)
    }

    /// The faces edge `e` bounds, once for each time a loop uses it (a periodic face's seam twice).
    pub fn edge_faces(&self, e: usize) -> Vec<usize> {
        self.record.edges.get(e).map_or_else(Vec::new, |edge| edge.uses.iter().map(|u| u.face as usize).collect())
    }

    /// Points of edge `e` from its start to its end, each with its parameter: the ends, and
    /// between them as many as keep the curve within `sag` mm of the chord between two
    /// neighbours and turning less than a twentieth of a turn between them. A line is its two
    /// ends.
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
        // before the left keeps the output in order. A closed edge is halved before it is looked at.
        let mut todo = vec![(t0, p0, t1, p1, 0u32)];
        while let Some((a, pa, b, pb, depth)) = todo.pop() {
            let m = 0.5 * (a + b);
            let pm = at(m)?;
            let chord = pb - pa;
            let off = if chord.len() > 0.0 { (pm - pa).cross(chord).len() / chord.len() } else { pm.dist(pa) };
            let bent = match (curve.derivs(a), curve.derivs(b)) {
                (Some((_, da)), Some((_, db))) => da.normalized().zip(db.normalized()).is_none_or(|(x, y)| x.dot(y) < turn),
                _ => false,
            };
            if (off > sag || bent || (depth == 0 && pa.dist(pb) <= sag)) && depth < MAX_DEPTH && out.len() + todo.len() < MAX_EDGE_POINTS {
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
    pub fn edge_distance(&self, e: usize, p: Vec3, sag: f64) -> Option<f64> {
        let points = self.edge_points(e, sag)?;
        let [t0, t1] = self.edge_interval(e)?;
        let (mut t, mut best) = points.iter().map(|(t, q)| (*t, q.dist(p))).min_by(|x, y| x.1.total_cmp(&y.1))?;
        for _ in 0..MAX_NEWTON {
            let Some((q, d)) = self.edge_at(e, t) else { break };
            best = best.min(q.dist(p));
            let dd = d.dot(d);
            if !(dd > 1e-300) {
                break;
            }
            let next = (t - (q - p).dot(d) / dd).clamp(t0, t1);
            if (next - t).abs() <= 1e-15 * (1.0 + t.abs()) {
                break;
            }
            t = next;
        }
        Some(best)
    }

    /// The parameters on face `f` of `p`, a point within `accept` mm of its surface, brought
    /// into the face's own domain where the surface turns round.
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
            if let (Some(period), Some([lo, hi])) = (periods.get(k).copied().flatten(), face.domain.get(k)) {
                let middle = 0.5 * (lo + hi);
                *slot += ((middle - *slot) / period).round() * period;
            }
        }
        Some(uv)
    }

    /// Face `f`'s unit normal at its parameters `uv`, out of the material: the natural normal,
    /// turned where the face's shell uses the face against it.
    pub fn face_normal(&self, f: usize, uv: [f64; 2]) -> Option<Vec3> {
        let (_, su, sv) = self.surfaces.get(f)?.as_ref()?.derivs(uv[0], uv[1])?;
        let n = su.cross(sv).normalized()?;
        Some(if self.forward.get(f).copied().flatten()? { n } else { -n })
    }

    /// Whether `p` lies on face `f`: within `accept` mm of its surface, and inside its loops
    /// (see [`Query::inside`]) or within `accept` mm of one of them in space (a face holds its
    /// own edges; see [`Query::on_loops`]).
    pub fn face_contains(&self, f: usize, p: Vec3, accept: f64) -> bool {
        self.face_uv(f, p, accept).is_some_and(|uv| self.inside(f, uv) == Some(true) || self.on_loops(f, p, accept))
    }

    /// The faces `p` lies on (see [`Query::face_contains`]), nearest first, each with how far
    /// `p` is from it.
    pub fn faces_at(&self, p: Vec3, accept: f64) -> Vec<(f64, usize)> {
        let mut out: Vec<(f64, usize)> = (0..self.record.faces.len())
            .filter_map(|f| {
                let uv = self.face_uv(f, p, accept)?;
                let q = self.surfaces.get(f)?.as_ref()?.point(uv)?;
                (self.inside(f, uv) == Some(true) || self.on_loops(f, p, accept)).then_some((q.dist(p), f))
            })
            .collect();
        out.sort_by(|x, y| x.0.total_cmp(&y.0));
        out
    }

    /// Where the ray from `o` along `d` meets the faces ahead of `o` (further than `accept`):
    /// each meeting's distance along `d` (a unit vector), face and point, nearest first. Faces on
    /// planes, cylinders, cones and spheres (a ball patch's sphere too) are met exactly. A face on
    /// a torus, a spline, a revolution or a blend is not met exactly here yet, and an answer
    /// without it could miss a meeting: `Err` names the first such face instead.
    pub fn ray(&self, o: Vec3, d: Vec3, accept: f64) -> Result<Vec<(f64, usize, Vec3)>, usize> {
        let Some(d) = d.normalized() else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for (f, face) in self.record.faces.iter().enumerate() {
            for s in ray_surface(&face.surface, o, d).ok_or(f)? {
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
    /// over all its edge loops: the outer loop holds the face and each hole holds a part of it the
    /// face leaves out, whatever order the loops are in. Each loop is the planar loop of its
    /// pcurves' Bézier pieces ([`planar`]), which the kernel says holds a point or not exactly, or
    /// refuses ([`loop_holds`]). `None` where it refuses (`uv` on a loop or within rounding of
    /// it) and where a pcurve can't be read so (malformed, or of a degree above
    /// [`MAX_BEZIER_DEGREE`]): [`Query::on_loops`] answers then.
    fn inside(&self, f: usize, uv: [f64; 2]) -> Option<bool> {
        let loops = self.trims.get(f)?.get_or_init(|| self.trim_loops(f)).as_ref()?;
        let p = Vec2::new(uv[0], uv[1]);
        let mut odd = false;
        for ring in loops {
            odd ^= match ring.iter().map(|c| planar(c, p)).collect::<Option<Vec<Seg2>>>() {
                Some(segs) => loop_holds(&Loop2 { segs }, p)?,
                // A piece of a degree no profile segment holds: the ring's crossings of a ray
                // from `p`, counted on its pieces themselves.
                None => ring.iter().map(|c| ray_crossings(c, p, MAX_HALVINGS)).sum::<Option<usize>>()? % 2 == 1,
            };
        }
        Some(odd)
    }

    /// Whether `p` lies within `accept` mm of one of face `f`'s loops, measured in space: from
    /// the curve of one of its edges ([`near`]), or a vertex loop's point. An edge with no NURBS
    /// form (a traced intersection) is measured by [`Query::edge_distance`].
    fn on_loops(&self, f: usize, p: Vec3, accept: f64) -> bool {
        let Some(face) = self.record.faces.get(f) else { return false };
        face.loops.iter().any(|l| match l {
            Loop::Edges { edges } => edges.iter().any(|oe| self.near_edge(oe.edge as usize, p, accept)),
            Loop::Vertex { vertex } => self.record.vertices.get(*vertex as usize).is_some_and(|v| v3(v.point).dist(p) <= accept),
        })
    }

    /// Whether `p` lies within `accept` mm of edge `e`'s curve (see [`Query::on_loops`]).
    fn near_edge(&self, e: usize, p: Vec3, accept: f64) -> bool {
        match self.forms.get(e).map(|cell| cell.get_or_init(|| self.edge_form(e))) {
            Some(Some(pieces)) => pieces.iter().any(|c| near(c, p, accept)),
            _ => self.edge_distance(e, p, accept).is_some_and(|d| d <= accept),
        }
    }

    /// Face `f`'s edge loops in its parameters, each the Bézier pieces of its pcurves in the order
    /// the loop runs (homogeneous `(u·w, v·w, 0, w)`). `None` where a pcurve is not a valid
    /// B-spline.
    fn trim_loops(&self, f: usize) -> Option<Vec<Pieces>> {
        let face = self.record.faces.get(f)?;
        let at = |x: f64, y: f64, w: f64| HPoint { x, y, z: 0.0, w };
        let mut out = Vec::new();
        for l in &face.loops {
            let Loop::Edges { edges } = l else { continue };
            let mut ring = Vec::new();
            for oe in edges {
                let (knots, ctrl): (&[f64], Vec<HPoint>) = match &oe.pcurve {
                    Curve2::Spline { knots, points } => (knots, points.iter().map(|q| at(q[0], q[1], 1.0)).collect()),
                    Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.iter().map(|h| at(h[0], h[1], h[2])).collect()),
                };
                let mut pieces = bezier_pieces(knots, ctrl, oe.pcurve.interval()?)?;
                if !oe.forward {
                    pieces.reverse();
                    pieces.iter_mut().for_each(|c| c.reverse());
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
        let line = |a: Vec3, b: Vec3| vec![HPoint::weighted(a, 1.0), HPoint::weighted(b, 1.0)];
        match edge.parameters() {
            Curve::Line { start, end } => {
                let (a, d) = (v3(*start), v3(*end) - v3(*start));
                Some(vec![line(a + d * t0, a + d * t1)])
            }
            Curve::Polyline { points } => {
                // Segment i spans [i, i + 1] of the parameter.
                let mut out = Vec::new();
                for (i, pair) in points.windows(2).enumerate() {
                    let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else { continue };
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

/// The Bézier pieces of spline `c` (a B-spline or NURBS record) over `interval`.
fn spline_pieces(c: &Curve, interval: [f64; 2]) -> Option<Pieces> {
    match c {
        Curve::Spline { knots, points } => bezier_pieces(knots, points.iter().map(|q| HPoint::weighted(v3(*q), 1.0)).collect(), interval),
        Curve::RationalSpline { knots, homogeneous } => {
            bezier_pieces(knots, homogeneous.iter().map(|h| HPoint { x: h[0], y: h[1], z: h[2], w: h[3] }).collect(), interval)
        }
        _ => None,
    }
}

/// The Bézier pieces over `[a, b]` of the B-spline on `knots` with homogeneous control points
/// `ctrl` ([`NurbsCurve::pieces`]), a piece cut where `[a, b]` ends inside it
/// ([`bezier_segment`]). `None` for a curve that is not a valid B-spline.
fn bezier_pieces(knots: &[f64], ctrl: Vec<HPoint>, [a, b]: [f64; 2]) -> Option<Pieces> {
    let degree = knots.len().checked_sub(ctrl.len() + 1)?;
    let curve = NurbsCurve::from_homogeneous(degree, knots.to_vec(), &ctrl).ok()?;
    let mut out = Vec::new();
    for piece in curve.pieces().ok()? {
        let [s0, s1] = piece.span();
        let (lo, hi) = (s0.max(a), s1.min(b));
        if !(lo < hi) {
            continue;
        }
        let ctrl: Vec<HPoint> = piece.ctrl().collect();
        out.push(if lo == s0 && hi == s1 { ctrl } else { bezier_segment(&ctrl, (lo - s0) / (s1 - s0), (hi - s0) / (s1 - s0)).ok()? });
    }
    Some(out)
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
fn planar(c: &[HPoint], p: Vec2) -> Option<Seg2> {
    let n = c.len().checked_sub(1).filter(|n| (1..=MAX_BEZIER_DEGREE).contains(n))?;
    let w0 = c.first()?.w;
    let rational = c.iter().any(|h| h.w != w0);
    let pts: Vec<Vec2> = if rational && n > 2 {
        c.iter().map(|h| p + Vec2::new(h.x - p.x * h.w, h.y - p.y * h.w)).collect()
    } else {
        c.iter().map(|h| Vec2::new(h.x / h.w, h.y / h.w)).collect()
    };
    Some(match (pts.as_slice(), c) {
        ([a, b], _) => Seg2::Line { a: *a, b: *b },
        ([a, apex, b], [h0, h1, h2]) => Seg2::Conic { a: *a, apex: *apex, b: *b, w: if rational { h1.w / (h0.w * h2.w).sqrt() } else { 1.0 } },
        ([p0, p1, p2, p3], _) => Seg2::Cubic { p0: *p0, p1: *p1, p2: *p2, p3: *p3 },
        _ => {
            let mut q = [Vec2::ZERO; MAX_BEZIER_CTRL];
            for (slot, x) in q.iter_mut().zip(&pts) {
                *slot = *x;
            }
            Seg2::Bezier { n: u8::try_from(n).ok()?, p: q }
        }
    })
}

/// How many times the ray from `p` along +x crosses the Bézier piece `ctrl` (homogeneous, its
/// weights above 0), an end on the ray's line counting as above it, as [`loop_holds`] reads ends.
/// Decided on the piece itself: its height above the line times its weight is a polynomial whose
/// Bernstein coefficients are `w_i (y_i - p.y)`, so with one sign among them the piece keeps to one
/// side, and with one change of sign it crosses the line once (variation diminishing), on the side
/// of `p` its hull lies on when all of the hull lies on one side. Anything else is halved, at most
/// `depth` times: `None` where that does not settle it (`p` on the piece, to rounding).
fn ray_crossings(ctrl: &[HPoint], p: Vec2, depth: u32) -> Option<usize> {
    if ctrl.iter().any(|h| !(h.w > 0.0)) {
        return None;
    }
    let above: Vec<bool> = ctrl.iter().map(|h| h.y - p.y * h.w >= 0.0).collect();
    let changes = above.windows(2).filter(|w| w.first() != w.last()).count();
    if changes == 0 || ctrl.iter().all(|h| h.x < p.x * h.w) {
        return Some(0);
    }
    if changes == 1 && ctrl.iter().all(|h| h.x > p.x * h.w) {
        return Some(1);
    }
    let depth = depth.checked_sub(1)?;
    let (a, b) = (bezier_segment(ctrl, 0.0, 0.5).ok()?, bezier_segment(ctrl, 0.5, 1.0).ok()?);
    Some(ray_crossings(&a, p, depth)? + ray_crossings(&b, p, depth)?)
}

/// Whether some point of the Bézier piece `ctrl` (homogeneous, its weights above 0) lies within
/// `reach` of `p`, decided on the piece itself. Its ends lie on it, and all of it within the hull
/// of its control points, so within the largest distance of a control point from its chord (the
/// segment between its ends) of that chord; a piece whose control points all lie on its chord (a
/// line) is its chord. A piece with an end within `reach` is near; one whose chord lies further
/// from `p` than `reach` and that distance is not; a line is near when its chord is; any other
/// is halved (de Casteljau) until one of those says. A piece halved [`MAX_HALVINGS`] times lies
/// within rounding of its chord, which is within `reach`, and so is near. Past [`MAX_PIECES`]
/// halvings in all (a curve that runs at about `reach` from `p` along much of its length, as a
/// circle of radius `reach` about `p` does) a piece is near when its chord lies within `reach`
/// and that distance.
fn near(ctrl: &[HPoint], p: Vec3, reach: f64) -> bool {
    let mut todo = vec![(ctrl.to_vec(), 0u32)];
    let mut halved = 0usize;
    while let Some((c, depth)) = todo.pop() {
        let Some(pts) = c.iter().map(|h| h.project()).collect::<Option<Vec<Vec3>>>() else { continue };
        let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else { continue };
        if a.dist(p) <= reach || b.dist(p) <= reach {
            return true;
        }
        let stray = pts.iter().map(|q| q.dist_to_segment(a, b)).fold(0.0, f64::max);
        let gap = p.dist_to_segment(a, b);
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
        let (Ok(left), Ok(right)) = (bezier_segment(&c, 0.0, 0.5), bezier_segment(&c, 0.5, 1.0)) else { continue };
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
            return if b.abs() > 1e-300 { vec![-c / b] } else { Vec::new() };
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
        if den.abs() <= 1e-15 * n.len() { Vec::new() } else { vec![(origin - o).dot(n) / den] }
    };
    let sphere = |center: Vec3, radius: f64| -> Vec<f64> {
        let w = o - center;
        roots(1.0, 2.0 * w.dot(d), w.dot(w) - radius * radius)
    };
    Some(match surface {
        Surface::Plane { origin, u_axis, v_axis } => plane(v3(*origin), v3(*u_axis).cross(v3(*v_axis))),
        Surface::PlaneThrough { origin, u_point, v_point } => {
            let c = v3(*origin);
            plane(c, (v3(*u_point) - c).cross(v3(*v_point) - c))
        }
        Surface::Cylinder { frame, radius } => {
            let (c, z) = (v3(frame.origin), v3(frame.x_axis).cross(v3(frame.y_axis)));
            let (w, dd) = ((o - c) - z * (o - c).dot(z), d - z * d.dot(z));
            roots(dd.dot(dd), 2.0 * w.dot(dd), w.dot(w) - radius * radius)
        }
        Surface::Cone { frame, radius_at_origin, half_angle } => {
            // r(v) = r0 + v tan(half), v along z; the radial distance squared is r(v)².
            let (c, z) = (v3(frame.origin), v3(frame.x_axis).cross(v3(frame.y_axis)));
            let k = half_angle.tan();
            let (w, dd) = ((o - c) - z * (o - c).dot(z), d - z * d.dot(z));
            let (h, dh) = ((o - c).dot(z), d.dot(z));
            let (r, dr) = (radius_at_origin + h * k, dh * k);
            roots(dd.dot(dd) - dr * dr, 2.0 * (w.dot(dd) - r * dr), w.dot(w) - r * r)
                .into_iter()
                .filter(|s| radius_at_origin + (h + dh * s) * k >= 0.0)
                .collect()
        }
        Surface::Sphere { frame, radius } => sphere(v3(frame.origin), *radius),
        Surface::BallPatch { center, radius } => sphere(v3(*center), *radius),
        Surface::Torus { .. }
        | Surface::Spline { .. }
        | Surface::RationalSpline { .. }
        | Surface::PlacedRevolution { .. }
        | Surface::Blend { .. } => return None,
    })
}

#[cfg(test)]
mod tests;
