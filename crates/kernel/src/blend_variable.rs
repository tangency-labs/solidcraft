//! Fillets beyond a constant radius: a radius that varies along the edge (from one end to the
//! other) and chord-length fillets (the width across the blend given instead of the radius).
//! SMLib builds both ([`crate::blend_smlib`]): a variable round as its variable-radius solver on a
//! B-spline radius law, a chord fillet as its constant-chord solver. On a straight edge between
//! two planar faces the band a radius law takes from the faces is checked against the faces
//! first, which also gives a [`taper`] its room along the edge.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::CurveEval;

use crate::blend::{Faces, too_large, unsupported};
use crate::body::Body;
use crate::{KernelError, Result, guard};

/// The two planar faces at an edge, the in-face directions away from it (`t1`, `t2`), the
/// angle between the directions.
struct EdgeFrame {
    i1: usize,
    i2: usize,
    t1: Vec3,
    t2: Vec3,
    phi: f64,
}

fn edge_frame(rec: &Faces, e: usize) -> Result<EdgeFrame> {
    let faces: Vec<usize> = rec.faces_of(e).into_iter().map(|f| f as usize).collect();
    let [i1, i2] = faces[..] else { return Err(unsupported("the edge is not between two faces")) };
    let (Some(n1), Some(n2)) = (rec.plane_normal(i1), rec.plane_normal(i2)) else {
        return Err(unsupported("both faces next to the edge must be planar"));
    };
    let into = |f: usize| -> Option<Vec3> {
        let mut uses = rec.uses(f).filter(|u| u.edge as usize == e);
        let u = uses.next()?;
        if uses.next().is_some() {
            return None;
        }
        rec.direction_into_face(f, u)
    };
    let (Some(t1), Some(t2)) = (into(i1), into(i2)) else { return Err(KernelError::Failed("blend: edge direction".into())) };
    let phi = t1.dot(t2).clamp(-1.0, 1.0).acos();
    let convex = n1.dot(t2) < -1e-9 && n2.dot(t1) < -1e-9;
    let concave = n1.dot(t2) > 1e-9 && n2.dot(t1) > 1e-9;
    if !(phi > 1e-3 && phi < std::f64::consts::PI - 1e-3) || !(convex || concave) {
        return Err(unsupported("the faces at the edge are tangent or the edge is ambiguous"));
    }
    Ok(EdgeFrame { i1, i2, t1, t2, phi })
}

/// Fillet one straight edge with radius `r0` at its end nearest `start` and `r1` at the other.
pub fn fillet_variable(body: &Body, edge_at: Vec3, start: Vec3, r0: f64, r1: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "fillet_variable");
    fillet_variable_points(body, edge_at, start, &[(0.0, r0), (1.0, r1)], false)
}

/// A fillet radius along an edge: a clamped B-spline of the position along it (0 at one end, 1
/// at the other), given by its degree, knots and control radii. The radius is a smooth curve
/// shaped by its control radii, each pulling it toward itself near its Greville position.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RadiusSpline {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub radii: Vec<f64>,
}

impl RadiusSpline {
    /// Through (position, radius) points sorted by position from 0 to 1: straight between them,
    /// or `smooth` (cubic pieces with Catmull-Rom slopes, C¹ through every point).
    pub fn through(pts: &[(f64, f64)], smooth: bool) -> RadiusSpline {
        if !smooth || pts.len() < 3 {
            let mut knots = vec![0.0];
            knots.extend(pts.iter().map(|p| p.0));
            knots.push(1.0);
            return RadiusSpline { degree: 1, knots, radii: pts.iter().map(|p| p.1).collect() };
        }
        let n = pts.len();
        let at = |i: usize| pts.get(i).copied().unwrap_or((0.0, 0.0));
        let slope = |i: usize| {
            let (a, b) = (at(i.saturating_sub(1)), at((i + 1).min(n - 1)));
            if b.0 > a.0 { (b.1 - a.1) / (b.0 - a.0) } else { 0.0 }
        };
        let mut knots = vec![0.0; 4];
        let mut radii = vec![at(0).1];
        for i in 0..n - 1 {
            let ((t0, r0), (t1, r1)) = (at(i), at(i + 1));
            let h = t1 - t0;
            radii.extend([r0 + slope(i) * h / 3.0, r1 - slope(i + 1) * h / 3.0, r1]);
            knots.extend(if i + 2 == n { vec![1.0; 4] } else { vec![t1; 3] });
        }
        RadiusSpline { degree: 3, knots, radii }
    }

    /// Greville abscissae: where each control radius sits along the edge.
    pub fn greville(&self) -> Vec<f64> {
        (0..self.radii.len()).map(|j| self.knots.iter().skip(j + 1).take(self.degree).sum::<f64>() / self.degree as f64).collect()
    }

    /// The radius at position `t`.
    pub fn eval(&self, t: f64) -> f64 {
        use solidcraft_geom::bspline::{Side, local_basis};
        let t = t.clamp(0.0, 1.0);
        // At the end of the domain the closing span's left limit holds.
        let side = if t >= 1.0 { Side::Left } else { Side::Right };
        self.radii.iter().enumerate().filter_map(|(j, r)| Some(local_basis(self.knots.get(j..=j + self.degree + 1)?, t, side, 0).first()? * r)).sum()
    }

    /// The same radius with its positions taken from the other end.
    pub fn reversed(&self) -> RadiusSpline {
        RadiusSpline {
            degree: self.degree,
            knots: self.knots.iter().rev().map(|k| 1.0 - k).collect(),
            radii: self.radii.iter().rev().copied().collect(),
        }
    }

    /// The same radius with one more control radius: the knot `u` inserted (Boehm), which leaves
    /// the curve as it was. None for `u` at or beyond an end, on an existing knot, or a spline
    /// already at 100 control radii.
    pub fn insert(&self, u: f64) -> Option<RadiusSpline> {
        let (n, p) = (self.radii.len(), self.degree);
        if !(u > 1e-6 && u < 1.0 - 1e-6) || n >= 100 || self.knots.iter().any(|k| (k - u).abs() < 1e-6) {
            return None;
        }
        // The span holding u: knots[k] <= u < knots[k + 1], p <= k < n.
        let k = (p..n).rev().find(|i| self.knots.get(*i).is_some_and(|x| *x <= u))?;
        let mut radii = Vec::with_capacity(n + 1);
        radii.extend_from_slice(self.radii.get(..=k - p)?);
        for i in k - p + 1..=k {
            let (lo, hi) = (*self.knots.get(i)?, *self.knots.get(i + p)?);
            let a = (u - lo) / (hi - lo);
            radii.push((1.0 - a) * self.radii.get(i - 1)? + a * self.radii.get(i)?);
        }
        radii.extend_from_slice(self.radii.get(k..)?);
        let mut knots = self.knots.clone();
        knots.insert(k + 1, u);
        Some(RadiusSpline { degree: self.degree, knots, radii })
    }

    /// The same spline without control radius `j` and the inner knot nearest its position: the
    /// curve changes around it, the ends stay. None for an end radius or a spline with no inner
    /// knot to remove.
    pub fn remove(&self, j: usize) -> Option<RadiusSpline> {
        let (n, p) = (self.radii.len(), self.degree);
        if j == 0 || j + 1 >= n || n <= p + 1 {
            return None;
        }
        let at = *self.greville().get(j)?;
        let inner = p + 1..n;
        let drop = inner.min_by(|a, b| (self.knots[*a] - at).abs().total_cmp(&(self.knots[*b] - at).abs()))?;
        let mut knots = self.knots.clone();
        knots.remove(drop);
        let mut radii = self.radii.clone();
        radii.remove(j);
        Some(RadiusSpline { degree: self.degree, knots, radii })
    }

    /// Refused unless it is a clamped B-spline from 0 to 1 of degree 1 to 3 with 2 to 100 control
    /// radii, positive along the edge and zero at most at one end.
    fn check(&self) -> Result<()> {
        let (n, deg) = (self.radii.len(), self.degree);
        if !(1..=3).contains(&deg) || !(2..=100).contains(&n) || n <= deg || self.knots.len() != n + deg + 1 {
            return Err(KernelError::Invalid("a radius spline has degree 1…3, 2…100 control radii and as many knots as both and one more".into()));
        }
        let clamped = self.knots.iter().take(deg + 1).all(|k| *k == 0.0) && self.knots.iter().skip(n).all(|k| *k == 1.0);
        if !clamped || !self.knots.iter().all(|k| k.is_finite()) || self.knots.windows(2).any(|w| w[1] < w[0]) {
            return Err(KernelError::Invalid("radius spline knots rise from 0 to 1, repeated degree + 1 times at each end".into()));
        }
        if self.radii.iter().any(|r| !(r.is_finite() && (*r == 0.0 || *r > 1e-6) && *r < 1e6)) {
            return Err(KernelError::Invalid("fillet radii must be positive (zero only at an end)".into()));
        }
        let zeros: Vec<usize> = (0..n).filter(|i| self.radii.get(*i) == Some(&0.0)).collect();
        if !matches!(zeros[..], [] | [0]) && zeros[..] != [n - 1] {
            return Err(KernelError::Invalid("the radius may be zero at one end of the edge only".into()));
        }
        // Positive inside the edge (a zero end is given exactly, as a control radius).
        if (1..200).any(|k| self.eval(k as f64 / 200.0) <= 1e-6) {
            return Err(KernelError::Invalid("the radius must stay positive along the edge".into()));
        }
        Ok(())
    }
}

/// Fillet one straight edge with a radius that varies along it: (position, radius) points with
/// positions 0 (the end nearest `start`) to 1 (the other end) and any between, the radius
/// straight between them or `smooth`. One end's radius may be zero.
pub fn fillet_variable_points(body: &Body, edge_at: Vec3, start: Vec3, points: &[(f64, f64)], smooth: bool) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "fillet_variable_points");
    if points.len() < 2 || points.len() > 100 {
        return Err(KernelError::Invalid("a variable radius needs 2…100 points".into()));
    }
    if points.iter().any(|(t, _)| !(t.is_finite() && (0.0..=1.0).contains(t))) {
        return Err(KernelError::Invalid("radius positions run from 0 to 1 along the edge".into()));
    }
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);
    if pts.first().map(|p| p.0) != Some(0.0) || pts.last().map(|p| p.0) != Some(1.0) || pts.len() < 2 {
        return Err(KernelError::Invalid("give the radius at both ends of the edge".into()));
    }
    fillet_variable_spline(body, edge_at, start, &RadiusSpline::through(&pts, smooth))
}

/// Fillet one edge with the radius `spline` along it, its positions from the end nearest
/// `start`. One end's radius may be zero. A law whose round would leave the faces beside the
/// edge is SMLib's refusal ([`KernelError::TooLarge`]).
pub fn fillet_variable_spline(body: &Body, edge_at: Vec3, start: Vec3, spline: &RadiusSpline) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "fillet_variable_spline");
    body.require_brep("fillet")?;
    spline.check()?;
    if !(edge_at.is_finite() && start.is_finite()) {
        return Err(KernelError::Invalid("the edge pick and the start point must be finite".into()));
    }
    crate::blend_smlib::fillet_variable(body, edge_at, start, spline)
}

/// A straight edge between two planar faces as the variable blend sees it: the body's record,
/// the frame at the edge, and its ends (start first).
struct EdgeSetup {
    rec: Faces,
    fr: EdgeFrame,
    edge: usize,
    v0: u32,
    v1: u32,
    p0: Vec3,
    p1: Vec3,
    d: Vec3,
    tol: f64,
}

impl EdgeSetup {
    /// The edge of `body` a pick at `at` takes, as the blend takes it.
    fn new(body: &Body, at: Vec3) -> Result<EdgeSetup> {
        let (picked, _) = crate::blend_smlib::picked_edges(body, &[at])?;
        let edge = *picked.first().ok_or_else(|| KernelError::Invalid(format!("no edge at {at:?}")))?;
        let rec = Faces::of(body)?;
        if !rec.straight(edge) {
            return Err(unsupported("only straight edges take a variable radius"));
        }
        let fr = edge_frame(&rec, edge)?;
        let [v0, v1] = rec.rec.edges.get(edge).ok_or_else(|| KernelError::Failed("edge not found".into()))?.vertices;
        let (p0, p1) = rec.ends(edge).ok_or_else(|| KernelError::Failed("edge not found".into()))?;
        let d = (p1 - p0).normalized().ok_or_else(|| KernelError::Failed("zero-length edge".into()))?;
        Ok(EdgeSetup { rec, fr, edge, v0, v1, p0, p1, d, tol: (body.size() * 1e-7).max(1e-9) })
    }

    /// How far a blend of radius `r` reaches into each face from the edge.
    fn back(&self, r: f64) -> f64 {
        r / (self.fr.phi / 2.0).tan()
    }

    /// Whether the band a blend of radius `r` takes from the faces at position `u` (strictly
    /// between the ends) stays inside both, clear of their other edges and holes.
    fn band_fits(&self, u: f64, r: f64) -> bool {
        let p = self.p0 + (self.p1 - self.p0) * u;
        let s = self.back(r);
        [(self.fr.i1, self.fr.t1), (self.fr.i2, self.fr.t2)].into_iter().all(|(i, t)| extension_fits(&self.rec, i, p, p + t * s, self.tol))
    }

    /// The largest radius up to `cap` whose band fits at position `u`.
    fn room(&self, u: f64, cap: f64) -> f64 {
        if self.band_fits(u, cap) {
            return cap;
        }
        let (mut lo, mut hi) = (0.0, cap);
        while hi - lo > self.tol {
            let mid = (lo + hi) / 2.0;
            if self.band_fits(u, mid) { lo = mid } else { hi = mid }
        }
        lo
    }

    /// The room at the end `v` (`out` pointing away from the edge there), as a radius: zero
    /// where a face closes to a point (its next side runs back along the edge), the reach of the
    /// sides along the faces where they are square to the edge, None where a side leans outward.
    fn end_room(&self, v: u32, out: Vec3) -> Option<f64> {
        let mut room = f64::INFINITY;
        for (i, t) in [(self.fr.i1, self.fr.t1), (self.fr.i2, self.fr.t2)] {
            let side = self
                .rec
                .uses(i)
                .map(|u| u.edge as usize)
                .find(|e| *e != self.edge && self.rec.rec.edges.get(*e).is_some_and(|x| x.vertices.contains(&v)))?;
            let [a, b] = self.rec.rec.edges.get(side)?.vertices;
            // The direction the side leaves `v` in.
            let dir = if a == v { self.rec.tangent(side, false)? } else { self.rec.tangent(side, true)? * -1.0 };
            let lean = dir.dot(out);
            if lean < -1e-9 {
                return Some(0.0);
            }
            if lean > 1e-9 || !self.rec.straight(side) {
                return None;
            }
            let other = if a == v { b } else { a };
            room = room.min((self.rec.point(other)? - self.rec.point(v)?).dot(t) * (self.fr.phi / 2.0).tan());
        }
        Some(room)
    }
}

/// The points of edge `e` from start to end: its ends for a line, else its curve divided until
/// each chord lies within `tol` of the curve at its middle (at most `MAX_POINTS`).
fn edge_points(rec: &Faces, e: usize, tol: f64) -> Option<Vec<Vec3>> {
    const MAX_POINTS: usize = 100_000;
    let (a, b) = rec.ends(e)?;
    if rec.straight(e) {
        return Some(vec![a, b]);
    }
    let edge = rec.rec.edges.get(e)?;
    let c = CurveEval::new(edge.parameters()).ok()?;
    let [t0, t1] = edge.interval;
    // Start from 8 pieces (a closed or turning curve's chord says nothing of its middle).
    let mut out = vec![a];
    let mut stack: Vec<(f64, Vec3, f64, Vec3, usize)> = Vec::new();
    let pieces = 8;
    let at = |k: usize| t0 + (t1 - t0) * k as f64 / pieces as f64;
    for k in (0..pieces).rev() {
        stack.push((at(k), c.point(at(k))?, at(k + 1), c.point(at(k + 1))?, 0));
    }
    while let Some((ta, pa, tb, pb, depth)) = stack.pop() {
        let tm = (ta + tb) / 2.0;
        let pm = c.point(tm)?;
        if depth < 24 && pm.dist_to_segment(pa, pb) > tol {
            stack.push((tm, pm, tb, pb, depth + 1));
            stack.push((ta, pa, tm, pm, depth + 1));
            continue;
        }
        out.push(pb);
        if out.len() > MAX_POINTS {
            return None;
        }
    }
    Some(out)
}

/// An extended corner edge must stay in the trimmed planar face `f`, not just its infinite
/// plane: the segment from `p` (on its boundary) to `q` crosses or touches no boundary after
/// `p`, holes included, and `q` is inside. Curved boundaries are divided more finely than the
/// clearance.
fn extension_fits(rec: &Faces, f: usize, p: Vec3, q: Vec3, tol: f64) -> bool {
    let tol = tol.max(4e-6);
    let (Some(x), Some(n)) = ((q - p).normalized(), rec.plane_normal(f)) else { return false };
    let y = n.cross(x);
    let len = p.dist(q);
    if !len.is_finite() || len <= tol {
        return false;
    }
    let mut inside = false;
    for u in rec.uses(f) {
        let Some(points) = edge_points(rec, u.edge as usize, tol * 0.25) else { return false };
        if points.len() < 2 {
            return false;
        }
        for pair in points.windows(2) {
            let [a, b] = pair else { return false };
            let a = *a - p;
            let b = *b - p;
            let (ax, ay, bx, by) = (a.dot(x), a.dot(y), b.dot(x), b.dot(y));
            if ![ax, ay, bx, by].iter().all(|v| v.is_finite()) {
                return false;
            }
            // Contact/overlap with the extension, except its original endpoint.
            if (ay.abs() <= tol && by.abs() <= tol && ax.max(bx) > tol && ax.min(bx) <= len + tol)
                || [(ax, ay), (bx, by)].iter().any(|(u, v)| *u > tol && *u <= len + tol && v.abs() <= tol)
            {
                return false;
            }
            // Points within the clearance of the ray count as on it, so a shared endpoint
            // contributes one crossing.
            let ay = if ay.abs() <= tol { 0.0 } else { ay };
            let by = if by.abs() <= tol { 0.0 } else { by };
            if (ay > 0.0) != (by > 0.0) {
                let hit = ax - ay * (bx - ax) / (by - ay);
                if hit > tol && hit <= len + tol {
                    return false;
                }
                // Even-odd containment of q over all loops (outer boundary and holes).
                if hit > len {
                    inside = !inside;
                }
            }
        }
    }
    inside
}

/// How much of the room along an edge a taper takes: its band stays within this fraction of each
/// face's width, so the faces keep a strip beside the round, narrowing to nothing only where a
/// face closes to a point.
const TAPER_SHARE: f64 = 0.9;
/// Spans of the cubic radius spline a taper builds.
const TAPER_SPANS: usize = 8;

/// The radius a taper gives one edge: its positions from `start`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Taper {
    pub start: Vec3,
    pub spline: RadiusSpline,
}

/// The radius spline that tapers `radius` on the straight edge at `edge_at` to the room along
/// it: `radius` where that fits, following the room down where it doesn't, to zero at an end
/// where a face closes to a point. Its control radii are the target min(radius, share × room)
/// at their Greville positions (Schoenberg's variation-diminishing spline), which stays under the
/// target wherever the room is concave along the edge (a convex face); the blend checks the rest.
pub fn taper(body: &Body, edge_at: Vec3, radius: f64) -> Result<Taper> {
    body.require_brep("fillet")?;
    if !(radius.is_finite() && radius > 1e-6 && radius < 1e6) {
        return Err(KernelError::Invalid("fillet radii must be positive".into()));
    }
    guard("taper", || {
        let s = EdgeSetup::new(body, edge_at)?;
        let leans = || unsupported("a taper needs each end of the edge square to it or closing to a point");
        let room0 = s.end_room(s.v0, s.d * -1.0).ok_or_else(leans)?;
        let room1 = s.end_room(s.v1, s.d).ok_or_else(leans)?;
        if room0 == 0.0 && room1 == 0.0 {
            return Err(too_large("the faces close to a point at both ends of the edge"));
        }
        let mut knots = vec![0.0; 4];
        knots.extend((1..TAPER_SPANS).map(|k| k as f64 / TAPER_SPANS as f64));
        knots.extend([1.0; 4]);
        let mut law = RadiusSpline { degree: 3, knots, radii: vec![0.0; TAPER_SPANS + 3] };
        let g = law.greville();
        let last = g.len() - 1;
        law.radii = g
            .iter()
            .enumerate()
            .map(|(j, u)| match j {
                0 => (TAPER_SHARE * room0).min(radius),
                j if j == last => (TAPER_SHARE * room1).min(radius),
                _ => TAPER_SHARE * s.room(*u, radius / TAPER_SHARE),
            })
            .collect();
        if law.radii.iter().take(last).skip(1).any(|r| *r <= 1e-6) {
            return Err(too_large("the room for the blend runs out inside the edge"));
        }
        law.check()?;
        Ok(Taper { start: s.p0, spline: law })
    })
}

/// A constant fillet of `edges` at `radius`; where that is too large, each edge it doesn't fit
/// on alone gets a [`taper`], and the rest the constant radius. The result and the taper of each
/// edge (in `edges`' order; None where the radius is constant).
pub fn fillet_tapered(body: &Body, edges: &[Vec3], radius: f64) -> Result<(Body, Vec<Option<Taper>>)> {
    let _span = solidcraft_profile::span("kernel", "fillet_tapered");
    let why = match crate::fillet(body, edges, radius) {
        Err(KernelError::TooLarge(why)) => why,
        other => return other.map(|b| (b, vec![None; edges.len()])),
    };
    let mut tapers = vec![None; edges.len()];
    let mut constant = Vec::new();
    for (p, slot) in edges.iter().zip(tapers.iter_mut()) {
        match crate::fillet(body, &[*p], radius) {
            Ok(_) => constant.push(*p),
            Err(KernelError::TooLarge(_)) => *slot = Some(taper(body, *p, radius)?),
            Err(e) => return Err(e),
        }
    }
    // Each edge fits alone but not together: nothing to taper.
    if tapers.iter().all(Option::is_none) {
        return Err(KernelError::TooLarge(why));
    }
    // Each step's result carries its history over `[body]`.
    let counts = crate::record::counts(&[body]);
    let mut cur = crate::boolean_record::operand(body, 0, &counts);
    for (p, t) in edges.iter().zip(&tapers) {
        if let Some(t) = t {
            cur = crate::record::after(fillet_variable_spline(&cur, *p, t.start, &t.spline)?, &cur, &counts);
        }
    }
    if !constant.is_empty() {
        cur = crate::record::after(crate::fillet(&cur, &constant, radius)?, &cur, &counts);
    }
    Ok((cur, tapers))
}

/// Chord-length fillet: each edge rounded so that the blend is `chord` wide across, from contact
/// to contact at every station (SMLib's constant-chord round), its radius following the face
/// angle along the edge.
pub fn fillet_chord(body: &Body, edges: &[Vec3], chord: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "fillet_chord");
    body.require_brep("fillet")?;
    if !(chord.is_finite() && chord > 1e-6 && chord < 1e6) {
        return Err(KernelError::Invalid("the chord length must be positive".into()));
    }
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    crate::blend_smlib::fillet_chord(body, edges, chord)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::{box_solid, measure};

    /// The number of edge uses of each face of `b`, and its vertex points.
    fn faces_and_vertices(b: &Body) -> (Vec<usize>, Vec<Vec3>) {
        let rec = crate::native::brep_record(b).unwrap();
        let sides = rec
            .faces
            .iter()
            .map(|f| {
                f.loops
                    .iter()
                    .map(|l| match l {
                        solidcraft_geom::brep::Loop::Edges { edges } => edges.len(),
                        solidcraft_geom::brep::Loop::Vertex { .. } => 0,
                    })
                    .sum()
            })
            .collect();
        (sides, rec.vertices.iter().map(|v| Vec3::new(v.point[0], v.point[1], v.point[2])).collect())
    }

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }

    /// Inserting a knot keeps the curve and adds one control radius; removing a control radius
    /// takes out one inner knot and keeps the ends.
    #[test]
    fn radius_spline_knot_insertion_and_removal() {
        let law =
            RadiusSpline { degree: 3, knots: vec![0.0, 0.0, 0.0, 0.0, 0.25, 0.6, 1.0, 1.0, 1.0, 1.0], radii: vec![2.0, 5.0, 1.5, 4.0, 1.0, 3.0] };
        for u in [0.1, 0.25 + 1e-3, 0.4, 0.9] {
            let more = law.insert(u).unwrap();
            assert_eq!((more.radii.len(), more.knots.len()), (7, 11));
            assert!(more.check().is_ok());
            let worst = (0..=400).map(|i| f64::from(i) / 400.0).map(|t| (more.eval(t) - law.eval(t)).abs()).fold(0.0, f64::max);
            assert!(worst < 1e-12, "insert {u}: off by {worst}");
            // Removing the control radius just added drops a knot again.
            let j = more.greville().iter().enumerate().min_by(|a, b| (a.1 - u).abs().total_cmp(&(b.1 - u).abs())).unwrap().0;
            let less = more.remove(j).unwrap();
            assert_eq!((less.radii.len(), less.knots.len()), (6, 10));
            assert!(less.check().is_ok());
        }
        // Not at an end, on a knot, or outside.
        assert!(law.insert(0.0).is_none() && law.insert(0.6).is_none() && law.insert(1.2).is_none());
        // Ends stay, and a single cubic span has no inner knot to give up.
        assert!(law.remove(0).is_none() && law.remove(5).is_none());
        let bare = RadiusSpline { degree: 3, knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], radii: vec![2.0, 3.0, 3.0, 2.0] };
        assert!(bare.remove(1).is_none());
        let less = law.remove(2).unwrap();
        assert_eq!((less.eval(0.0), less.eval(1.0)), (2.0, 3.0));
    }

    /// The material a variable fillet removes from a right-angled edge, radius running r0 → r1:
    /// its balls' union (`removed_by_balls`; the balls' great circles, (1 − π/4) L (r0² + r0 r1 +
    /// r1²) / 3, take more), to 1e-4 of itself.
    #[test]
    fn variable_radius_on_a_box_edge() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        // The edge along y at x = 10, z = 30; radius 2 at y = 0, 5 at y = 20.
        let f = fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0), 2.0, 5.0).unwrap();
        let removed = removed_by_balls(20.0, &|u| 2.0 + 3.0 * u, &open, &[]);
        let v = measure(&f).unwrap().volume;
        assert!(rel(6000.0 - v, removed) < 1e-4, "{} removed vs {removed}", 6000.0 - v);
        // Started from the other end: the same solid.
        let h = fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 20.0, 30.0), 5.0, 2.0).unwrap();
        assert!(rel(measure(&h).unwrap().volume, v) < 1e-9);
        // Equal radii give the constant fillet.
        let g = fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0), 3.0, 3.0).unwrap();
        let k = crate::fillet(&b, &[Vec3::new(10.0, 10.0, 30.0)], 3.0).unwrap();
        assert!(rel(measure(&g).unwrap().volume, measure(&k).unwrap().volume) < 1e-6);
        // Wider than the faces: past r = 10 the contact on the top leaves it across its far edge
        // x = 0 (past r = 30 the one on the side across its bottom), cliffs. The round is built as
        // if the faces went on, and what it removes is cut from the box: the balls' union inside
        // the box's section, 10 along the top and 30 down the side (the great circles' would take
        // 2453.81, near twice as much).
        let w = fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0), 2.0, 50.0).unwrap();
        let removed_w = removed_by_balls(20.0, &|u| 2.0 + 48.0 * u, &slab, &[20.0 * 8.0 / 48.0, 20.0 * 28.0 / 48.0]);
        let vw = measure(&w).unwrap().volume;
        assert!(rel(6000.0 - vw, removed_w) < 1e-4, "{} removed vs {removed_w}", 6000.0 - vw);
    }

    /// A radius through a middle point, straight pieces 2 → 4 → 2: a corner in the law, where the
    /// ball there shows between its two sides' touch circles; the balls' union takes 39.541034
    /// (the halves' great circles would take 40.0590).
    #[test]
    fn variable_radius_with_a_corner_through_a_middle_point() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let (at, start) = (Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0));
        let f = fillet_variable_points(&b, at, start, &[(0.0, 2.0), (0.5, 4.0), (1.0, 2.0)], false).unwrap();
        let removed = removed_by_balls(20.0, &|u| 4.0 - 4.0 * (u - 0.5).abs(), &open, &[10.0]);
        assert!((removed - 39.541034).abs() < 1e-5, "{removed}");
        let v = measure(&f).unwrap().volume;
        assert!(rel(6000.0 - v, removed) < 1e-4, "{} removed vs {removed}", 6000.0 - v);
        assert_eq!(f.unmeshed_faces(), 0);
    }

    /// A radius through a middle point, smooth: the removed volume is the balls' union.
    #[test]
    fn variable_radius_through_a_middle_point() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let (at, start) = (Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0));
        let pts = [(0.0, 2.0), (0.5, 4.0), (1.0, 2.0)];
        let s = fillet_variable_points(&b, at, start, &pts, true).unwrap();
        let law = RadiusSpline::through(&pts, true);
        assert!((law.eval(0.5) - 4.0).abs() < 1e-12 && (law.eval(0.0) - 2.0).abs() < 1e-12);
        let removed = removed(&law, 20.0);
        let v = measure(&s).unwrap().volume;
        assert!(rel(6000.0 - v, removed) < 1e-4, "{} removed vs {removed}", 6000.0 - v);
        // Unsorted, from the other end: the same.
        let r = fillet_variable_points(&b, at, Vec3::new(10.0, 20.0, 30.0), &[(1.0, 2.0), (0.0, 2.0), (0.5, 4.0)], true).unwrap();
        assert!(rel(measure(&r).unwrap().volume, v) < 1e-9);
        // A middle radius too big for the faces, missing ends, bad positions: refused.
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (0.5, 40.0), (1.0, 2.0)], false).is_err());
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (0.5, 4.0)], false).is_err());
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (f64::NAN, 4.0), (1.0, 2.0)], false).is_err());
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (1.0, -1.0)], false).is_err());
    }

    /// The radius running down to zero at one end: the blend closes to the corner, a three-sided
    /// face, and the end face there is untouched. Removed: the balls' union (a cone to its apex
    /// when linear, 22.594877; the great circles' (1 − π/4) L r0² / 3 would take 22.8945).
    #[test]
    fn variable_radius_down_to_zero_at_one_end() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let (at, start, end) = (Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0), Vec3::new(10.0, 20.0, 30.0));
        let cone = removed_by_balls(20.0, &|u| 4.0 - 4.0 * u, &open, &[]);
        assert!((cone - 22.594877).abs() < 1e-5, "{cone}");
        for (r0, r1) in [(4.0, 0.0), (0.0, 4.0)] {
            let f = fillet_variable(&b, at, start, r0, r1).unwrap();
            let v = measure(&f).unwrap().volume;
            assert!(rel(6000.0 - v, cone) < 1e-4, "{r0} → {r1}: {} removed vs {cone}", 6000.0 - v);
            assert_eq!(f.unmeshed_faces(), 0);
            let (faces, vertices) = faces_and_vertices(&f);
            assert_eq!(faces.len(), 7);
            // The blend is three-sided; the closed end's corner is still a box corner.
            assert!(faces.contains(&3));
            let corner = if r1 == 0.0 { end } else { start };
            assert!(vertices.iter().any(|x| x.dist(corner) < 1e-9));
        }
        // Through a middle point, smooth, closing at the far end.
        let pts = [(0.0, 3.0), (0.5, 4.0), (1.0, 0.0)];
        let s = fillet_variable_points(&b, at, start, &pts, true).unwrap();
        let law = RadiusSpline::through(&pts, true);
        let removed = removed(&law, 20.0);
        let v = measure(&s).unwrap().volume;
        assert!(rel(6000.0 - v, removed) < 1e-4, "{} removed vs {removed}", 6000.0 - v);
        // Zero at both ends, zero in the middle, a smooth radius dipping below zero, or a radius
        // too small to build: refused.
        assert!(fillet_variable(&b, at, start, 0.0, 0.0).is_err());
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (0.5, 0.0), (1.0, 2.0)], false).is_err());
        let dip = [(0.0, 0.0), (0.2, 0.1), (0.4, 6.0), (1.0, 6.0)];
        assert!((1..200).any(|i| RadiusSpline::through(&dip, true).eval(i as f64 / 200.0) <= 1e-6));
        assert!(fillet_variable_points(&b, at, start, &dip, true).is_err());
        assert!(fillet_variable(&b, at, start, 4.0, 1e-9).is_err());
    }

    /// A prism of the polygon `pts` (in xy) from z = `z0` to `z1`.
    fn prism_between(pts: &[(f64, f64)], z0: f64, z1: f64) -> Body {
        use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
        let outer = Loop2::polygon(&pts.iter().map(|(x, y)| Vec2::new(*x, *y)).collect::<Vec<_>>());
        crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], z0, z1).unwrap().pop().unwrap()
    }

    /// A prism of the polygon `pts` (in xy) from z = 0 to `h`.
    fn prism(pts: &[(f64, f64)], h: f64) -> Body {
        prism_between(pts, 0.0, h)
    }

    /// A plate 15 high whose top narrows from 20 to 2 mm along 40 mm (the trapezoid (0, 0),
    /// (40, 0), (40, 2), (0, 20)), against a wall standing behind its slanted side from z = -15
    /// to 30: the plate's top and bottom end on the wall in concave edges, so a blend of its
    /// front edges can't run past them (no cliff to cut over). 6600 + 20 · 38 · 45.
    fn ledge() -> Body {
        let plate = prism(&[(0.0, 0.0), (40.0, 0.0), (40.0, 2.0), (0.0, 20.0)], 15.0);
        let wall = prism_between(&[(40.0, 2.0), (0.0, 20.0), (0.0, 30.0), (40.0, 30.0)], -15.0, 30.0);
        crate::boolean(&plate, &wall, crate::BoolOp::Union).unwrap().unwrap()
    }

    /// Simpson's rule for `f` on [a, b] in `n` (even) panels.
    fn simpson(f: impl Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
        let h = (b - a) / n as f64;
        (0..=n)
            .map(|i| {
                f(a + i as f64 * h)
                    * if i == 0 || i == n {
                        1.0
                    } else if i % 2 == 1 {
                        4.0
                    } else {
                        2.0
                    }
            })
            .sum::<f64>()
            * h
            / 3.0
    }

    /// The material a variable round removes from a right-angled edge of length `len`: what no
    /// ball reaches, the round being the union of its balls (their envelope; each ball touches it
    /// along the circle square to the line of centres, not its great circle, as the radius
    /// varies). The ball at s along the edge has radius `law(s / len)` (the law going on past the
    /// ends along its end slopes) and its centre that far from both faces. In each plane square
    /// to the edge at x, the ray from the edge at angle θ from the first face meets the union
    /// first at the least entry over the balls, and the corner up to there is taken, as far as
    /// `room(x, θ)` (the body's section along the ray; ∞ where it does not end): ∫ ρ²/2 dθ,
    /// integrated over x, both by Simpson's rule, split at `kinks` (where the law turns).
    pub(crate) fn removed_by_balls(len: f64, law: &dyn Fn(f64) -> f64, room: &dyn Fn(f64, f64) -> f64, kinks: &[f64]) -> f64 {
        let h = 1e-7;
        let (k0, k1) = ((law(h) - law(0.0)) / (h * len), (law(1.0) - law(1.0 - h)) / (h * len));
        let r = |s: f64| {
            if s < 0.0 {
                law(0.0) + k0 * s
            } else if s > len {
                law(1.0) + k1 * (s - len)
            } else {
                law(s / len)
            }
        };
        let big = (0..=1000).map(|i| law(i as f64 / 1000.0)).fold(0.0f64, f64::max);
        let entry = |x: f64, c: f64, sn: f64, s: f64| {
            let rr = r(s);
            let p = rr * (c + sn);
            let disc = p * p - (x - s) * (x - s) - rr * rr;
            if rr > 0.0 && disc >= 0.0 { p - disc.sqrt() } else { f64::INFINITY }
        };
        let rho = |x: f64, t: f64| {
            let (c, sn) = (t.cos(), t.sin());
            let (lo, hi, scan) = (x - 1.5 * big, x + 1.5 * big, 400);
            let step = (hi - lo) / scan as f64;
            let (mut best, mut at) = (f64::INFINITY, x);
            for i in 0..=scan {
                let s = lo + i as f64 * step;
                let e = entry(x, c, sn, s);
                if e < best {
                    (best, at) = (e, s);
                }
            }
            let (mut a, mut b, g) = (at - step, at + step, (5f64.sqrt() - 1.0) / 2.0);
            for _ in 0..80 {
                let (m1, m2) = (b - g * (b - a), a + g * (b - a));
                if entry(x, c, sn, m1) < entry(x, c, sn, m2) {
                    b = m2;
                } else {
                    a = m1;
                }
            }
            best.min(entry(x, c, sn, 0.5 * (a + b)))
        };
        // The rays meet the union tangentially at the faces (ρ goes as the root of the angle
        // there): θ = π/4 (1 − cos(π q)) makes the integrand smooth in q.
        let area = |x: f64| {
            let q = std::f64::consts::PI;
            simpson(
                |u: f64| {
                    let t = q / 4.0 * (1.0 - (q * u).cos());
                    let p = rho(x, t).min(room(x, t));
                    let p = if p.is_finite() { p } else { 0.0 };
                    0.5 * p * p * q * q / 4.0 * (q * u).sin()
                },
                0.0,
                1.0,
                128,
            )
        };
        let mut ends = vec![0.0];
        ends.extend(kinks.iter().copied().filter(|k| *k > 0.0 && *k < len));
        ends.push(len);
        ends.windows(2).map(|w| simpson(area, w[0], w[1], 2 * ((64.0 * (w[1] - w[0]) / len).ceil() as usize).max(1))).sum()
    }

    /// No end to the body's section: the faces go on past the round.
    fn open(_x: f64, _t: f64) -> f64 {
        f64::INFINITY
    }

    /// The 10 x 20 x 30 box's section about its edge x = 10, z = 30: 10 along the top, 30 down
    /// the side.
    fn slab(_x: f64, t: f64) -> f64 {
        (10.0 / t.cos().max(1e-300)).min(30.0 / t.sin().max(1e-300))
    }

    /// The material a right-angled blend with the radius `law` removes along an edge of length L
    /// where the faces go on past it: the union of its balls.
    fn removed(law: &RadiusSpline, len: f64) -> f64 {
        removed_by_balls(len, &|u| law.eval(u), &open, &[])
    }

    /// The volume of a fine tessellation: at a blend closed to a point `measure` takes a
    /// coarser mesh of the blend, a few mm³ off on these parts.
    fn fine_volume(b: &Body) -> f64 {
        b.mesh(&crate::MeshTolerance::chord(5e-4)).unwrap().measure().volume
    }

    /// A taper on the top front edge of a wedge, whose top face narrows to a point, and of a
    /// plate against a wall, whose top face narrows to 2 mm: the radius asked for where it fits,
    /// below 0.9 of the room everywhere, down to zero at the wedge's tip and to 1.8 mm at the
    /// plate's narrow end; the volume integrates the spline. On the free-standing wedge R16 runs
    /// over the top's far edge and the front's bottom edge (cliffs), so it is built as if those
    /// faces went on and cut from the wedge (the blend-overflow decision, 237f0ae74), and a
    /// tapered fillet keeps it; against the wall the plate's R3 has no room at the narrow end
    /// and tapers.
    #[test]
    fn taper_follows_the_room_along_the_edge() {
        let (at, start) = (Vec3::new(20.0, 0.0, 15.0), Vec3::new(0.0, 0.0, 15.0));
        let plate = ledge();
        let full = 6600.0 + 20.0 * 38.0 * 45.0;
        assert!(matches!(crate::fillet(&plate, &[at], 3.0), Err(KernelError::TooLarge(_))));
        let (f, tapers) = fillet_tapered(&plate, &[at], 3.0).unwrap();
        let t = tapers.into_iter().flatten().next().unwrap();
        let law = if t.start.dist(start) < 1e-9 { t.spline } else { t.spline.reversed() };
        assert!((law.eval(0.0) - 3.0).abs() < 1e-9 && (law.eval(1.0) - 1.8).abs() < 1e-9);
        let v = fine_volume(&f);
        assert!(rel(v, full - removed(&law, 40.0)) < 1e-4, "{v} vs {}", full - removed(&law, 40.0));
        // A radius that fits stays constant.
        let (g, tapers) = fillet_tapered(&plate, &[at], 1.0).unwrap();
        assert_eq!(tapers, vec![None]);
        assert!(rel(measure(&g).unwrap().volume, measure(&crate::fillet(&plate, &[at], 1.0).unwrap()).unwrap().volume) < 1e-12);

        let wedge = prism(&[(0.0, 0.0), (40.0, 0.0), (0.0, 20.0)], 15.0);
        // At x the section is the rectangle [0, w] × [0, 15], w = 20 (1 − x/40), less the part
        // of the 16 × 16 square at the edge outside the circle about (16, 16): integrated along x.
        let cut = |w: f64| {
            let (r, s, t) = (16.0f64, 16.0f64.min(w), 15.0);
            let f = |v: f64| 0.5 * (v * (r * r - v * v).max(0.0).sqrt() + r * r * (v / r).clamp(-1.0, 1.0).asin());
            let s1 = (r - (r * r - (r - t) * (r - t)).sqrt()).min(s);
            t * s1 + r * (s - s1) - (f(r - s1) - f(r - s))
        };
        let n = 20000;
        let removed16: f64 = (0..n).map(|i| cut(20.0 * (1.0 - (i as f64 + 0.5) / n as f64))).sum::<f64>() * 40.0 / n as f64;
        let (f, tapers) = fillet_tapered(&wedge, &[at], 16.0).unwrap();
        assert_eq!(tapers, vec![None]);
        let v = measure(&f).unwrap().volume;
        assert!(rel(v, 6000.0 - removed16) < 1e-6, "{v} vs {}", 6000.0 - removed16);
        // The taper the room gives: the top face is 20 (1 − u) wide, the front 15 high.
        let t = taper(&wedge, at, 16.0).unwrap();
        let law = if t.start.dist(start) < 1e-9 { t.spline.clone() } else { t.spline.reversed() };
        for i in 0..=100 {
            let u = i as f64 / 100.0;
            let r = law.eval(u);
            assert!(r <= 0.9 * (20.0 * (1.0 - u)).min(15.0) + 1e-9 && r <= 16.0, "{u}: {r}");
        }
        assert_eq!(law.eval(1.0), 0.0);
        assert!((law.eval(0.0) - 13.5).abs() < 1e-9);
        let f = fillet_variable_spline(&wedge, at, t.start, &t.spline).unwrap();
        let v = fine_volume(&f);
        assert!(rel(v, 6000.0 - removed(&law, 40.0)) < 1e-4, "{v} vs {}", 6000.0 - removed(&law, 40.0));
        assert!(f.validity().is_empty(), "{:?}", f.validity());
    }

    /// A radius spline given directly: a cubic with its control radii pulling the radius up in
    /// the middle; the same from the other end; refused when it isn't clamped or dips to zero.
    #[test]
    fn variable_radius_from_a_spline() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let (at, start) = (Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0));
        let law = RadiusSpline { degree: 3, knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0], radii: vec![2.0, 2.0, 6.0, 3.0, 0.0] };
        let f = fillet_variable_spline(&b, at, start, &law).unwrap();
        let v = fine_volume(&f);
        assert!(rel(v, 6000.0 - removed(&law, 20.0)) < 1e-4, "{v} vs {}", 6000.0 - removed(&law, 20.0));
        let r = fillet_variable_spline(&b, at, Vec3::new(10.0, 20.0, 30.0), &law.reversed()).unwrap();
        assert!(rel(fine_volume(&r), v) < 1e-9);
        let unclamped = RadiusSpline { knots: vec![0.0, 0.0, 0.0, 0.1, 0.5, 1.0, 1.0, 1.0, 1.0], ..law.clone() };
        assert!(fillet_variable_spline(&b, at, start, &unclamped).is_err());
        let dips = RadiusSpline { radii: vec![2.0, 0.0, 0.0, 3.0, 2.0], ..law.clone() };
        assert!(fillet_variable_spline(&b, at, start, &dips).is_err());
        // A middle radius up to 21.25 (on the top 10 wide), rising and falling so fast that the
        // balls' envelope folds over itself at the peak: for the balls' family
        // F(y, p) = |p - c(y)|² - r(y)² the touch arc there has F_yy down to -8.88 (where the touch
        // circles move back along the edge). Refused, never built folded (the balls' great
        // circles, which never fold, built it to 726.53 removed against the union's 332.2).
        let big = RadiusSpline { radii: vec![2.0, 2.0, 40.0, 3.0, 2.0], ..law };
        assert!(matches!(fillet_variable_spline(&b, at, start, &big), Err(KernelError::TooLarge(_))));
    }

    #[test]
    fn chord_length_on_a_right_angle() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        // A right angle: chord c gives radius c / √2.
        let f = fillet_chord(&b, &[Vec3::new(10.0, 10.0, 30.0)], 4.0).unwrap();
        let k = crate::fillet(&b, &[Vec3::new(10.0, 10.0, 30.0)], 4.0 / 2f64.sqrt()).unwrap();
        assert!(rel(measure(&f).unwrap().volume, measure(&k).unwrap().volume) < 1e-9);
    }
}

#[cfg(test)]
#[path = "taper_mesh_tests.rs"]
mod taper_mesh_tests;
