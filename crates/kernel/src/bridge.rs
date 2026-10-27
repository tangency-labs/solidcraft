//! Continuity-driven bridging surfaces: one B-spline face that leaves one surface and arrives at
//! another with a requested continuity (G0 up to G3, [`Construction::Bridge`]). This is the core the Blend, Boundary and
//! Match surfaces are built on.
//!
//! A bridge is a tensor product. Across it (v) every station's cross-section is a Bézier whose
//! leading control points reproduce the support surface's derivatives — the Hermite rows — so the
//! finished face carries the requested continuity by construction: one control row per condition
//! (position, tangent, curvature, change of curvature). Along it (u) those cross-sections are
//! interpolated by a cubic, so the face passes through every station exactly. Every condition is
//! stated against the support surface's own derivatives measured along the direction the bridge
//! leaves in, which makes G2 the curvature vector and G3 the way that curvature vector changes
//! along that direction (its normal part: what a surface-to-surface match can hold).
//!
//! What the support surface does along the leave direction is read off from samples on its own
//! parameter line (the line through the station that leaves in the leave direction at unit
//! speed) by finite differences: the second derivative of a surface curve with a unit tangent is
//! its curvature vector, and the third is the arclength derivative of that curvature vector.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::Brep;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::bspline::{CurveN, HPoint, NurbsSurface};

use crate::body::Body;
use crate::continuity_order::Construction;
pub use crate::continuity_order::Continuity;
use crate::surfaces::Chain;
use crate::{KernelError, Result};

/// One station of a boundary: where the bridge starts, the direction it leaves in, and what the
/// support surface does there along that direction.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// The point on the boundary.
    pub p: Vec3,
    /// Unit direction the bridge leaves *towards the other boundary*, in the tangent plane of
    /// the support surface at `p`.
    pub leave: Vec3,
    /// The support surface's curvature vector along `leave` (zero on a plane).
    pub bend: Vec3,
    /// The arclength derivative of `bend` along `leave` (zero on a plane).
    pub bend_change: Vec3,
}

/// The edges of `body` picked by points on them, chained: each runs on from the last, and the
/// chain may close. Each is an edge of the body's record (its index) with whether the chain runs
/// along it. Used with [`frames`], whose chain must be a chain of one face's own edges.
pub fn chain(body: &Body, at: &[Vec3]) -> Result<Chain> {
    body.require_brep("blend")?;
    if at.is_empty() || at.len() > 10_000 || at.iter().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid("pick 1 to 10000 points on the edges of a chain".into()));
    }
    let r = body.record();
    let mut pool: Vec<usize> = Vec::new();
    for p in at {
        let e = crate::surfaces::edge_near(body, *p).ok_or_else(|| KernelError::Invalid("no edge near a picked point".to_string()))?;
        if !pool.contains(&e) {
            pool.push(e);
        }
    }
    let ends = |e: usize| r.edges.get(e).map(|x| x.vertices).ok_or_else(|| KernelError::Invalid("an edge reference".into()));
    let first = pool.remove(0);
    let mut chain = vec![(first, true)];
    // The picks need not be given in order: run forward from the first edge, then backwards.
    let mut end = ends(first)?[1];
    while let Some(k) = pool.iter().position(|e| ends(*e).is_ok_and(|v| v.contains(&end))) {
        let e = pool.remove(k);
        let [a, z] = ends(e)?;
        let forward = a == end;
        end = if forward { z } else { a };
        chain.push((e, forward));
    }
    let mut start = ends(first)?[0];
    while let Some(k) = pool.iter().position(|e| ends(*e).is_ok_and(|v| v.contains(&start))) {
        let e = pool.remove(k);
        let [a, z] = ends(e)?;
        let forward = z == start;
        start = if forward { a } else { z };
        chain.insert(0, (e, forward));
    }
    if !pool.is_empty() {
        return Err(KernelError::Invalid("the picked edges do not run on from one another".into()));
    }
    Ok(chain)
}

/// Whether a chain of `body`'s edges closes back on itself.
pub fn chain_closes(body: &Body, chain: &Chain) -> bool {
    closes(body.record(), chain)
}

fn closes(r: &Brep, chain: &Chain) -> bool {
    let end = |(e, forward): (usize, bool), head: bool| r.edges.get(e).map(|x| if forward == head { x.vertices[1] } else { x.vertices[0] });
    let (Some(first), Some(last)) = (chain.first(), chain.last()) else { return false };
    end(*first, false).is_some() && end(*first, false) == end(*last, true)
}

/// A point of a [`ChainLine`]'s polyline: where it is, its arclength along the chain, the chain
/// edge (its index in the chain) the segment from it runs along, and that edge's curve parameter
/// there.
#[derive(Clone, Copy)]
struct Sample {
    p: Vec3,
    s: f64,
    edge: usize,
    t: f64,
}

/// A chain of edges measured by arclength through a fine polyline on it, so that a station is read
/// on the edge's own curve, not on a chord. A chain that closes ends back on its first point. Each
/// edge's curve is walked the way the chain runs along it.
struct ChainLine {
    curves: Vec<(CurveEval, [f64; 2])>,
    samples: Vec<Sample>,
    closed: bool,
}

impl ChainLine {
    fn of(body: &Body, chain: &Chain) -> Result<ChainLine> {
        /// Samples per edge used for the length table.
        const PER_EDGE: usize = 64;
        let r = body.record();
        let mut samples: Vec<Sample> = Vec::with_capacity(chain.len() * (PER_EDGE + 1) + 1);
        let mut curves = Vec::with_capacity(chain.len());
        let push = |samples: &mut Vec<Sample>, p: Vec3, edge: usize, t: f64| {
            let s = samples.last().map_or(0.0, |last| last.s + last.p.dist(p));
            samples.push(Sample { p, s, edge, t });
        };
        let fail = || KernelError::Failed("bridge: an edge has no point".into());
        for (i, &(e, forward)) in chain.iter().enumerate() {
            let edge = r.edges.get(e).ok_or_else(|| KernelError::Invalid("an edge reference".into()))?;
            let c = CurveEval::new(edge.parameters()).map_err(|err| KernelError::Failed(format!("bridge: {err}")))?;
            let [t0, t1] = edge.interval;
            let (a, z) = if forward { (t0, t1) } else { (t1, t0) };
            for k in 0..=PER_EDGE {
                // The segment that starts at this point: along this edge, or the zero-length join
                // to the next edge.
                let t = a + (z - a) * k as f64 / PER_EDGE as f64;
                push(&mut samples, c.point(t).ok_or_else(fail)?, i, t);
            }
            curves.push((c, [a, z]));
        }
        let closed = closes(r, chain);
        if closed && let Some(first) = samples.first().copied() {
            push(&mut samples, first.p, chain.len().saturating_sub(1), first.t);
        }
        let line = ChainLine { curves, samples, closed };
        let total = line.length();
        if !(total > 1e-9) || !total.is_finite() {
            return Err(KernelError::Invalid("the edge chain has no length".into()));
        }
        Ok(line)
    }

    fn length(&self) -> f64 {
        self.samples.last().map_or(0.0, |x| x.s)
    }

    fn point(&self, edge: usize, t: f64) -> Result<Vec3> {
        self.curves.get(edge).and_then(|(c, _)| c.point(t)).ok_or_else(|| KernelError::Failed("bridge: sample".into()))
    }

    /// The point at arclength `s` (wrapped round a closed chain, clamped on an open one), on its
    /// edge's own curve, and the chain edge it is on.
    fn at(&self, s: f64) -> Result<(Vec3, usize)> {
        let total = self.length();
        let s = if self.closed { s.rem_euclid(total) } else { s.clamp(0.0, total) };
        let hi = self.samples.partition_point(|x| x.s < s).clamp(1, self.samples.len().saturating_sub(1));
        let fail = || KernelError::Failed("bridge: sample".into());
        let (lo, hi) = (self.samples.get(hi - 1).ok_or_else(fail)?, self.samples.get(hi).ok_or_else(fail)?);
        let span = hi.s - lo.s;
        if !(span > 1e-12) {
            // The zero-length join between two edges: the end of the edge it belongs to.
            return Ok((self.point(lo.edge, lo.t)?, lo.edge));
        }
        let f = ((s - lo.s) / span).clamp(0.0, 1.0);
        Ok((self.point(lo.edge, lo.t + (hi.t - lo.t) * f)?, lo.edge))
    }

    /// `count` stations evenly along the chain, from arclength `start` (a closed chain's seam; an
    /// open chain always runs end to end), forwards or backwards. A closed chain's last station is
    /// back on its first.
    fn stations(&self, count: usize, start: f64, backwards: bool) -> Result<Vec<(Vec3, usize)>> {
        let total = self.length();
        let start = if self.closed {
            start
        } else if backwards {
            total
        } else {
            0.0
        };
        let sign = if backwards { -1.0 } else { 1.0 };
        (0..count).map(|i| self.at(start + sign * total * i as f64 / (count.max(2) - 1) as f64)).collect()
    }

    /// The chain edges `p` (a station on chain edge `edge`) lies on: that edge, and the edge before
    /// or after it when `p` is on the vertex they share.
    fn edges_at(&self, p: Vec3, edge: usize) -> [Option<usize>; 3] {
        let count = self.curves.len();
        let tol = self.length() * 1e-9 + 1e-12;
        let Some((curve, [a, z])) = self.curves.get(edge) else { return [Some(edge), None, None] };
        let at = |t: f64| curve.point(t).is_some_and(|q| q.dist(p) <= tol);
        let before = at(*a).then(|| edge.checked_sub(1).or(self.closed.then(|| count.saturating_sub(1)))).flatten();
        let after = at(*z).then(|| Some(edge + 1).filter(|e| *e < count).or(self.closed.then_some(0))).flatten();
        [Some(edge), before, after]
    }

    /// Where stations start from `seam` (a point on the chain), and whether they run backwards: a
    /// closed chain starts at it, an open one runs from the end it is nearer.
    fn start_at_seam(&self, seam: Vec3) -> (f64, bool) {
        let s = self.nearest(seam);
        if self.closed { (s, false) } else { (0.0, s > self.length() / 2.0) }
    }

    /// The arclength of the chain's point nearest `p`, and how far `p` is from the chain: the
    /// nearest point of the fine polyline, then refined on that edge's own curve between the
    /// samples either side (golden-section search).
    fn nearest_with_distance(&self, p: Vec3) -> (f64, f64) {
        let mut best = (f64::INFINITY, 0.0, 0usize, 0.0, 0.0);
        for w in self.samples.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            let len = a.p.dist(b.p);
            let f = if len > 1e-12 { ((p - a.p).dot(b.p - a.p) / (len * len)).clamp(0.0, 1.0) } else { 0.0 };
            let d = p.dist(a.p + (b.p - a.p) * f);
            if d < best.0 && a.edge == b.edge {
                best = (d, a.s + len * f, a.edge, a.t, b.t);
            }
        }
        let (chord, s, edge, ta, tb) = best;
        let on_curve = self.curves.get(edge).and_then(|(c, _)| {
            let d = |t: f64| c.point(t).map_or(f64::INFINITY, |q| q.dist(p));
            let (mut lo, mut hi) = (ta.min(tb), ta.max(tb));
            let g = 0.5 * (5f64.sqrt() - 1.0);
            for _ in 0..80 {
                let (x, y) = (hi - g * (hi - lo), lo + g * (hi - lo));
                if d(x) < d(y) {
                    hi = y;
                } else {
                    lo = x;
                }
            }
            Some(d(0.5 * (lo + hi))).filter(|x| x.is_finite())
        });
        (s, on_curve.map_or(chord, |d| d.min(chord)))
    }

    /// The arclength of the chain's point nearest `p`.
    fn nearest(&self, p: Vec3) -> f64 {
        self.nearest_with_distance(p).0
    }

    /// The middle of the chain: a closed chain's centroid (what it surrounds), an open chain's
    /// point halfway along.
    fn middle(&self) -> Result<Vec3> {
        if !self.closed {
            return Ok(self.at(self.length() / 2.0)?.0);
        }
        let mut sum = Vec3::ZERO;
        let mut weight = 0.0;
        for w in self.samples.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            let len = a.p.dist(b.p);
            sum += (a.p + b.p) * (0.5 * len);
            weight += len;
        }
        Ok(sum * (1.0 / weight.max(1e-300)))
    }
}

/// The support surface's frames along `chain` (a chain of edges of `body`'s face `face`), for a
/// bridge that leaves in direction `leave` (projected into the tangent plane at each station).
pub fn frames(body: &Body, face: usize, chain: &Chain, leave: Vec3, count: usize) -> Result<Vec<Frame>> {
    if !(2..=4097).contains(&count) {
        return Err(KernelError::Invalid("a bridge needs 2 to 4097 stations".into()));
    }
    let line = ChainLine::of(body, chain)?;
    let stations = line.stations(count, 0.0, false)?;
    let r = body.record();
    frames_on(r, &vec![face; chain.len()], &stations, &line, leave)
}

/// The frames at `stations` (points on `line`, each with the index of the chain edge it is on),
/// each read on the support face of its own edge (`faces`, faces of `r`, one per chain edge): a
/// chain may run along the boundaries of several faces, a tube's rim along the halves of its wall.
/// A station on the vertex where two of those faces meet sits at a corner of one face's
/// parameters, where the shape may not be readable along the leave direction; it is then read on
/// the other face, which holds the same point.
fn frames_on(r: &Brep, faces: &[usize], stations: &[(Vec3, usize)], line: &ChainLine, leave: Vec3) -> Result<Vec<Frame>> {
    let leave =
        leave.normalized().filter(|l| l.is_finite()).ok_or_else(|| KernelError::Invalid("the leave direction must be a finite vector".into()))?;
    let along = |n: Vec3| -> Result<Vec3> {
        (leave - n * leave.dot(n))
            .normalized()
            .ok_or_else(|| KernelError::Invalid("the leave direction is normal to the support surface: it has no tangent plane there".to_string()))
    };
    let length = line.length();
    // One support per distinct face, each with its own walk (a parameter hint means nothing on
    // another surface).
    let mut supports: Vec<(usize, Support, Walk)> = Vec::new();
    let mut out = Vec::with_capacity(stations.len());
    'stations: for (p, edge) in stations {
        let mut tried: Vec<usize> = Vec::with_capacity(2);
        let mut refusal: Option<KernelError> = None;
        for e in line.edges_at(*p, *edge).into_iter().flatten() {
            let Some(&face) = faces.get(e) else { continue };
            if tried.contains(&face) {
                continue;
            }
            tried.push(face);
            let k = match supports.iter().position(|(f, _, _)| *f == face) {
                Some(k) => k,
                None => {
                    supports.push((face, Support::of(r, face, length)?, Walk::new(length)));
                    supports.len() - 1
                }
            };
            let Some((_, support, walk)) = supports.get_mut(k) else { continue };
            match support.station(*p, &along, walk, true) {
                Ok((frame, _)) => {
                    out.push(frame);
                    continue 'stations;
                }
                // The station's own face's refusal is the one reported when the other cannot read
                // it either.
                Err(err) => {
                    refusal.get_or_insert(err);
                }
            }
        }
        return Err(refusal.unwrap_or_else(|| KernelError::Failed("bridge: a station's edge has no support face".into())));
    }
    Ok(out)
}

/// The frames of `body`'s face `face` at stations on its own boundary, each leaving the face
/// straight across that boundary: `stations` are points on the boundary with the boundary's
/// direction there *as the face's own boundary runs* (counter-clockwise about its outward normal),
/// and the leave direction is that direction × the face's outward normal — in the face's tangent
/// plane, at right angles to the boundary, away from the face. `length` is the size of the boundary
/// the stations are on (it sets how closely a station must lie on the surface and the step the
/// shape is read with). Each frame comes with the face's outward unit normal there. With `shape`
/// false only the leave direction and the normal are read (`bend` and `bend_change` are zero),
/// which needs no room on the surface beside the station.
pub fn frames_across(body: &Body, face: usize, stations: &[(Vec3, Vec3)], length: f64, shape: bool) -> Result<Vec<(Frame, Vec3)>> {
    if stations.len() > 1 << 16 || !(length.is_finite() && length > 0.0) {
        return Err(KernelError::Invalid("frames across a boundary need at most 65536 stations on a boundary of finite size".into()));
    }
    let r = body.record();
    let support = Support::of(r, face, length)?;
    let outward = if support.forward { 1.0 } else { -1.0 };
    let mut walk = Walk::new(length);
    let mut out = Vec::with_capacity(stations.len());
    for (p, tangent) in stations {
        if !p.is_finite() || !tangent.is_finite() {
            return Err(KernelError::Invalid("a boundary station must be finite".into()));
        }
        let away = |n: Vec3| -> Result<Vec3> {
            tangent.cross(n * outward).normalized().ok_or_else(|| KernelError::Invalid("the boundary runs along the face's normal".to_string()))
        };
        let (frame, normal) = support.station(*p, &away, &mut walk, shape)?;
        out.push((frame, normal * outward));
    }
    Ok(out)
}

/// A support surface read at stations: the surface, where its parameters run (a periodic
/// direction without bound), whether its face is used along its natural normal, and how close a
/// station must lie to it.
struct Support {
    surf: SurfaceEval,
    range: [(f64, f64); 2],
    forward: bool,
    tol: f64,
}

/// What reading one station hands on to the next: the station's parameters (the hint for the
/// next projection, which keeps the search on the same side of a periodic or split surface) and
/// the finite-difference step that worked.
struct Walk {
    hint: Option<[f64; 2]>,
    step: f64,
}

impl Walk {
    fn new(length: f64) -> Walk {
        // Finite-difference step: a thousandth of the chain, so curvature over the chain is read
        // locally but round-off stays far below the geometry.
        Walk { hint: None, step: (length * 1e-3).clamp(1e-6, 0.5) }
    }
}

impl Support {
    fn of(r: &Brep, face: usize, length: f64) -> Result<Support> {
        let f = r.faces.get(face).ok_or_else(|| KernelError::Invalid("no such face".into()))?;
        let surf = SurfaceEval::new(f.parameters()).map_err(|e| KernelError::Failed(format!("bridge: {e}")))?;
        let dom = surf.domain();
        let range = [0, 1].map(|d| match (dom.period[d], dom.range[d]) {
            (None, Some(r)) => r,
            _ => (f64::NEG_INFINITY, f64::INFINITY),
        });
        let forward = r
            .shells
            .iter()
            .find_map(|s| match s {
                solidcraft_geom::brep::Shell::Faces { faces, .. } => faces.iter().find(|o| o.face as usize == face).map(|o| o.forward),
                _ => None,
            })
            .unwrap_or(true);
        // A face's own edges may sit a fit tolerance off its surface, so the projection is only
        // asked to be close, not exact.
        let tol = (length * 1e-4).max(1e-6);
        Ok(Support { surf, range, forward, tol })
    }

    /// The frame at `p`, leaving in `leave(n)` (`n` the surface's natural unit normal there), and
    /// that normal. Without `read_shape`, the frame's shape is left at zero.
    fn station(&self, p: Vec3, leave: &dyn Fn(Vec3) -> Result<Vec3>, walk: &mut Walk, read_shape: bool) -> Result<(Frame, Vec3)> {
        let (surf, [range_u, range_v], tol) = (&self.surf, self.range, self.tol);
        // The station before this one is the hint where it lands close; a chain that wraps the
        // surface (or a periodic one) falls back to a free search.
        let Some([u, v]) = surf.project(p, walk.hint, tol).or_else(|| surf.project(p, None, tol)) else {
            return Err(KernelError::Invalid("a picked edge is not on the support surface".into()));
        };
        walk.hint = Some([u, v]);
        let fail = || KernelError::Failed("bridge: the support surface has no point there".into());
        let (_, su, sv) = surf.derivs(u, v).ok_or_else(fail)?;
        let n = su.cross(sv).normalized().ok_or_else(|| KernelError::Failed("bridge: the support surface has no normal".into()))?;
        let w = leave(n)?;
        if !read_shape {
            return Ok((Frame { p, leave: w, bend: Vec3::ZERO, bend_change: Vec3::ZERO }, n));
        }
        // Parameter-space direction that leaves in `w` at unit 3D speed: S_u·du + S_v·dv = w.
        let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let det = a * c - b * b;
        let scale = a.max(c).max(1e-30);
        if !(det.abs() > 1e-12 * scale * scale) {
            return Err(KernelError::Failed("bridge: the support surface is singular at the boundary".into()));
        }
        let du = (c * w.dot(su) - b * w.dot(sv)) / det;
        let dv = (a * w.dot(sv) - b * w.dot(su)) / det;
        // The samples of the parameter line. A station sits on the support surface's own
        // boundary, so the line usually leaves the face: the shape is then read from the samples
        // of the one side that is still on the surface, which is the surface's own extension past
        // that boundary (exact for a plane and a cylinder, what a blend continues from).
        let slack = 1e-9;
        let on = |t: f64, h: f64| {
            let (x, y) = (u + t * h * du, v + t * h * dv);
            x >= range_u.0 - slack && x <= range_u.1 + slack && y >= range_v.0 - slack && y <= range_v.1 + slack
        };
        let central = [-2.0, -1.0, 0.0, 1.0, 2.0];
        let forward = [0.0, 1.0, 2.0, 3.0, 4.0];
        let backward = [0.0, -1.0, -2.0, -3.0, -4.0];
        let pick = |h: f64| -> Option<[f64; 5]> {
            if central.iter().all(|t| on(*t, h)) {
                Some(central)
            } else if forward.iter().all(|t| on(*t, h)) {
                Some(forward)
            } else if backward.iter().all(|t| on(*t, h)) {
                Some(backward)
            } else {
                None
            }
        };
        let mut h = walk.step;
        let mut chosen = pick(h);
        for _ in 0..8 {
            if chosen.is_some() {
                break;
            }
            h *= 0.25;
            chosen = pick(h);
        }
        let Some(offsets) = chosen else {
            return Err(KernelError::Invalid("the support surface is too small around a picked edge to read its shape".into()));
        };
        let q: Vec<Vec3> = offsets.iter().map(|t| surf.point([u + t * h * du, v + t * h * dv]).ok_or_else(fail)).collect::<Result<_>>()?;
        let (Some(&f0), Some(&f1), Some(&f2), Some(&f3), Some(&f4)) = (q.first(), q.get(1), q.get(2), q.get(3), q.get(4)) else {
            return Err(KernelError::Failed("bridge: samples".into()));
        };
        // Central samples read the line both ways; one-sided samples read it one way only from the
        // station, backwards down it in the second case (the odd derivatives then change sign).
        let (d1, d2, d3) = if offsets[0] == 0.0 && offsets[1] < 0.0 {
            let d1 = (f0 * -25.0 + f1 * 48.0 + f2 * -36.0 + f3 * 16.0 + f4 * -3.0) * (-1.0 / (12.0 * h));
            let d2 = (f0 * 35.0 + f1 * -104.0 + f2 * 114.0 + f3 * -56.0 + f4 * 11.0) * (1.0 / (12.0 * h * h));
            let d3 = (f0 * -5.0 + f1 * 18.0 + f2 * -24.0 + f3 * 14.0 + f4 * -3.0) * (-1.0 / (2.0 * h * h * h));
            (d1, d2, d3)
        } else if offsets[0] == 0.0 {
            let d1 = (f0 * -25.0 + f1 * 48.0 + f2 * -36.0 + f3 * 16.0 + f4 * -3.0) * (1.0 / (12.0 * h));
            let d2 = (f0 * 35.0 + f1 * -104.0 + f2 * 114.0 + f3 * -56.0 + f4 * 11.0) * (1.0 / (12.0 * h * h));
            let d3 = (f0 * -5.0 + f1 * 18.0 + f2 * -24.0 + f3 * 14.0 + f4 * -3.0) * (1.0 / (2.0 * h * h * h));
            (d1, d2, d3)
        } else {
            let d1 = (f0 * -1.0 + f1 * 8.0 + f3 * -8.0 + f4) * (1.0 / (12.0 * h));
            let d2 = (f0 + f1 * -16.0 + f2 * 30.0 + f3 * -16.0 + f4) * (1.0 / (12.0 * h * h));
            let d3 = (f0 * -1.0 + f1 * 2.0 + f3 * -2.0 + f4) * (1.0 / (2.0 * h * h * h));
            (d1, d2, d3)
        };
        let (t, bend, bend_change) = shape(d1, d2, d3)?;
        // Read the same curve the other way when the surface's own parameter direction runs
        // against the leave direction: the curvature vector is the same either way, the change of
        // curvature turns round with the direction of travel.
        let bend_change = if t.dot(w) < 0.0 { -bend_change } else { bend_change };
        walk.step = h;
        Ok((Frame { p, leave: w, bend, bend_change }, n))
    }
}

/// The geometric shape of a surface curve at a point from its first three derivatives along the
/// curve: the unit tangent, the curvature vector and the arclength derivative of that curvature
/// vector. All three are what a surface-to-surface continuity condition is stated with.
fn shape(d1: Vec3, d2: Vec3, d3: Vec3) -> Result<(Vec3, Vec3, Vec3)> {
    let sigma = d1.len();
    if !(sigma > 1e-12) || !sigma.is_finite() {
        return Err(KernelError::Failed("bridge: the support surface's parameter line does not move".into()));
    }
    let t = d1 * (1.0 / sigma);
    let tp = d2 * (1.0 / sigma) - d1 * (d1.dot(d2) / (sigma * sigma * sigma));
    let nn = d2 - t * d2.dot(t);
    let np = d3 - t * d3.dot(t) - tp * d2.dot(t) - t * d2.dot(tp);
    let kappa = np * (sigma * sigma) - nn * (2.0 * d1.dot(d2));
    let bend_change = kappa * (1.0 / (sigma * sigma * sigma * sigma * sigma));
    if ![t, nn, bend_change].into_iter().all(Vec3::is_finite) {
        return Err(KernelError::Failed("bridge: the support surface's shape is not finite at the boundary".into()));
    }
    Ok((t, nn * (1.0 / (sigma * sigma)), bend_change))
}

/// The third derivative a curve needs at a boundary so that the arclength derivative of its
/// curvature vector is `target`, from its first two derivatives there. Only the normal-plane
/// part of the third derivative can hold that condition; the rest is a reparameterisation of the
/// cross-section and is left at zero.
fn third_derivative(d1: Vec3, d2: Vec3, target: Vec3) -> Result<Vec3> {
    let sigma = d1.len();
    if !(sigma > 1e-12) || !sigma.is_finite() {
        return Err(KernelError::Failed("bridge: no room for the curvature condition".into()));
    }
    let t = d1 * (1.0 / sigma);
    let tp = d2 * (1.0 / sigma) - d1 * (d1.dot(d2) / (sigma * sigma * sigma));
    let nn = d2 - t * d2.dot(t);
    let r = tp * (-d2.dot(t)) - t * d2.dot(tp);
    let x = (target * (sigma.powi(5)) - r * (sigma * sigma) + nn * (2.0 * d1.dot(d2))) * (1.0 / (sigma * sigma));
    if !x.is_finite() {
        return Err(KernelError::Failed("bridge: the curvature condition is not finite".into()));
    }
    Ok(x - t * x.dot(t))
}

/// One station's cross-section: a Bézier whose leading control points hold the conditions of
/// each side. `speed` is how fast the cross-section leaves each boundary (any positive value; it
/// shapes the middle without touching the conditions).
fn section(a: &Frame, b: &Frame, orders: [Continuity; 2], n: usize, speed: [f64; 2]) -> Result<Vec<Vec3>> {
    let (Some(speed_a), Some(speed_b)) = (speed.first().copied(), speed.get(1).copied()) else {
        return Err(KernelError::Failed("bridge: speed".into()));
    };
    let (ka, kb) = (orders[0].conditions(), orders[1].conditions());
    if ka + kb - 1 != n {
        return Err(KernelError::Failed("bridge: degree".into()));
    }
    let mut c = vec![Vec3::ZERO; n + 1];
    if let Some(slot) = c.get_mut(0) {
        *slot = a.p;
    }
    if let Some(slot) = c.get_mut(n) {
        *slot = b.p;
    }
    let k = n as f64;
    // The first end: one control point per condition.
    if ka >= 2 {
        let p1 = a.p + a.leave * (speed_a / k);
        if let Some(slot) = c.get_mut(1) {
            *slot = p1;
        }
        if ka >= 3 {
            if n < 2 {
                return Err(KernelError::Failed("bridge: degree".into()));
            }
            let p2 = a.bend * (speed_a * speed_a / (k * (k - 1.0))) + p1 * 2.0 - a.p;
            if let Some(slot) = c.get_mut(2) {
                *slot = p2;
            }
            if ka >= 4 {
                if n < 3 {
                    return Err(KernelError::Failed("bridge: degree".into()));
                }
                let d1 = (p1 - a.p) * k;
                let d2 = (a.p - p1 * 2.0 + p2) * (k * (k - 1.0));
                let a3 = third_derivative(d1, d2, a.bend_change)?;
                if let Some(slot) = c.get_mut(3) {
                    *slot = a3 * (1.0 / (k * (k - 1.0) * (k - 2.0))) + p2 * 3.0 - p1 * 3.0 + a.p;
                }
            }
        }
    }
    // The second end, from its own conditions read backwards.
    if kb >= 2 {
        let q1 = b.p + b.leave * (speed_b / k);
        if let Some(slot) = c.get_mut(n - 1) {
            *slot = q1;
        }
        if kb >= 3 {
            if n < 2 {
                return Err(KernelError::Failed("bridge: degree".into()));
            }
            let q2 = b.bend * (speed_b * speed_b / (k * (k - 1.0))) + q1 * 2.0 - b.p;
            if let Some(slot) = c.get_mut(n - 2) {
                *slot = q2;
            }
            if kb >= 4 {
                if n < 3 {
                    return Err(KernelError::Failed("bridge: degree".into()));
                }
                let d1 = (q1 - b.p) * k;
                let d2 = (b.p - q1 * 2.0 + q2) * (k * (k - 1.0));
                let q3 = third_derivative(d1, d2, b.bend_change)?;
                if let Some(slot) = c.get_mut(n - 3) {
                    *slot = q3 * (1.0 / (k * (k - 1.0) * (k - 2.0))) + q2 * 3.0 - q1 * 3.0 + b.p;
                }
            }
        }
    }
    if c.iter().any(|p| !p.is_finite()) {
        return Err(KernelError::Failed("bridge: the cross-section is not finite".into()));
    }
    Ok(c)
}

/// The bridge surface between two boundaries of `stations` frames each: one B-spline face
/// leaving `a` and arriving at `b` with the requested continuity. `crown` scales how fast the
/// cross-sections leave each boundary (0: the plain bridge, up to 3: fuller, down to -0.9:
/// flatter), which shapes the middle without touching the conditions at either end.
pub fn bridge(a: &[Frame], b: &[Frame], orders: [Continuity; 2], crown: [f64; 2]) -> Result<Body> {
    let (Some(&crown_a), Some(&crown_b)) = (crown.first(), crown.get(1)) else { return Err(KernelError::Failed("bridge: crown".into())) };
    if a.len() != b.len() || a.len() < 2 || a.len() > 4097 {
        return Err(KernelError::Invalid("a bridge needs 2 to 4097 stations, the same on both sides".into()));
    }
    for c in [crown_a, crown_b] {
        if !c.is_finite() || !(-0.9..=3.0).contains(&c) {
            return Err(KernelError::Invalid("a bridge's crown must be between -0.9 and 3".into()));
        }
    }
    for g in orders {
        g.built_by(Construction::Bridge)?;
    }
    let stations = a.len();
    let n = orders[0].conditions() + orders[1].conditions() - 1;
    if n == 0 || n > solidcraft_geom::bspline::MAX_SPLINE_DEGREE {
        return Err(KernelError::Failed("bridge: degree".into()));
    }
    let mut layers: Vec<Vec<Vec3>> = vec![Vec::with_capacity(stations); n + 1];
    for (fa, fb) in a.iter().zip(b) {
        if !fa.p.is_finite() || !fb.p.is_finite() || !fa.leave.is_finite() || !fb.leave.is_finite() {
            return Err(KernelError::Invalid("a bridge station must be finite".into()));
        }
        let chord = fa.p.dist(fb.p);
        if !(chord > 1e-9) {
            return Err(KernelError::Invalid("the two boundaries meet: there is no room for a bridge".into()));
        }
        let speed = [chord / 3.0 * (1.0 + crown_a), chord / 3.0 * (1.0 + crown_b)];
        let controls = section(fa, fb, orders, n, speed)?;
        for (layer, c) in layers.iter_mut().zip(controls) {
            layer.push(c);
        }
    }
    // Along the bridge: a cubic through the stations' cross-sections, so the face passes through
    // each of them exactly.
    let params: Vec<f64> = (0..stations).map(|i| i as f64 / (stations - 1) as f64).collect();
    let degree = 3.min(stations - 1);
    let mut knots_u: Option<Vec<f64>> = None;
    let mut ctrl = vec![Vec3::ZERO; (n + 1) * stations];
    for (j, layer) in layers.iter().enumerate() {
        let points: Vec<[f64; 3]> = layer.iter().map(|p| [p.x, p.y, p.z]).collect();
        let curve = CurveN::<3>::interpolate(&params, &points, degree).map_err(|e| KernelError::Failed(format!("bridge: {e}")))?;
        if knots_u.is_none() {
            knots_u = Some(curve.knots.clone());
        }
        for (i, c) in curve.ctrl.iter().enumerate() {
            if let Some(slot) = ctrl.get_mut(j * stations + i) {
                *slot = Vec3::new(c[0], c[1], c[2]);
            }
        }
    }
    let knots_u = knots_u.ok_or_else(|| KernelError::Failed("bridge: knots".into()))?;
    let mut knots_v = vec![0.0; n + 1];
    knots_v.extend(std::iter::repeat_n(1.0, n + 1));
    let points: Vec<HPoint> = ctrl.iter().map(|p| HPoint::weighted(*p, 1.0)).collect();
    let nurbs = NurbsSurface::from_homogeneous([degree, n], [knots_u, knots_v], points).map_err(|e| KernelError::Failed(format!("bridge: {e}")))?;
    crate::surfaces::tensor_sheet(&nurbs)
}

/// The one face of a bridge body: its surface and its parameter box.
pub(crate) fn the_face(body: &Body) -> Result<(SurfaceEval, [[f64; 2]; 2])> {
    let r = body.record();
    let face = r.faces.first().ok_or_else(|| KernelError::Failed("bridge: no face".into()))?;
    let surf = SurfaceEval::new(face.parameters()).map_err(|e| KernelError::Failed(format!("bridge: {e}")))?;
    Ok((surf, face.domain))
}

/// The surface of a bridge face as a grid of points over its parameter box, for callers that
/// measure the result.
pub fn sample_grid(body: &Body, along: usize, across: usize) -> Result<Vec<Vec<Vec3>>> {
    if !(2..=1025).contains(&along) || !(2..=1025).contains(&across) {
        return Err(KernelError::Invalid("a grid needs 2 to 1025 samples each way".into()));
    }
    let (surf, [[u0, u1], [v0, v1]]) = the_face(body)?;
    let fail = || KernelError::Failed("bridge: the surface has no point there".into());
    (0..along)
        .map(|i| {
            let u = u0 + (u1 - u0) * i as f64 / (along - 1) as f64;
            (0..across).map(|j| surf.point([u, v0 + (v1 - v0) * j as f64 / (across - 1) as f64]).ok_or_else(fail)).collect()
        })
        .collect()
}

/// The most any point of a bridge's boundary row is from the chain it was built on — the row at
/// `v0` for the first boundary, at `v1` for the second: what a stitch would have to close up.
fn boundary_gap(made: &Body, line: &ChainLine, at_end: bool) -> Result<f64> {
    let (surf, [[u0, u1], [v0, v1]]) = the_face(made)?;
    let v = if at_end { v1 } else { v0 };
    let mut worst: f64 = 0.0;
    for i in 0..=256 {
        let q =
            surf.point([u0 + (u1 - u0) * i as f64 / 256.0, v]).ok_or_else(|| KernelError::Failed("bridge: the surface has no point there".into()))?;
        worst = worst.max(line.nearest_with_distance(q).1);
    }
    Ok(worst)
}

/// Which face of `body` the blend continues from along each edge of a chain (one per edge, in
/// chain order, by its index in the body's record): of the faces each edge bounds, the one whose
/// tangent plane holds as much of `leave` as possible there. A chain may run along several faces
/// (a tube's rim along the halves of its wall).
fn support_faces(r: &Brep, chain: &Chain, leave: Vec3, size: f64) -> Result<Vec<usize>> {
    let mut out = Vec::with_capacity(chain.len());
    for &(e, _) in chain {
        let edge = r.edges.get(e).ok_or_else(|| KernelError::Invalid("an edge reference".into()))?;
        let mid = CurveEval::new(edge.parameters())
            .ok()
            .and_then(|c| c.point(0.5 * (edge.interval[0] + edge.interval[1])))
            .ok_or_else(|| KernelError::Failed("bridge: an edge has no point".into()))?;
        let mut best: Option<(f64, usize)> = None;
        for u in &edge.uses {
            let face = u.face as usize;
            let Some(surf) = r.faces.get(face).and_then(|f| SurfaceEval::new(f.parameters()).ok()) else { continue };
            let Some(uv) = surf.project(mid, None, size * 1e-6 + 1e-9) else { continue };
            let Some(n) = surf.derivs(uv[0], uv[1]).and_then(|(_, su, sv)| su.cross(sv).normalized()) else { continue };
            let score = (leave - n * leave.dot(n)).len();
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, face));
            }
        }
        let (score, face) = best.ok_or_else(|| KernelError::Invalid("no face of the body holds a picked edge".into()))?;
        if score < 0.2 {
            return Err(KernelError::Invalid(
                "the direction towards the other boundary lies in no face at the picked edges: give `align` points to leave by".into(),
            ));
        }
        out.push(face);
    }
    Ok(out)
}

/// Where the second chain's stations start, and which way they run, so that they pair with the
/// first chain's `a` station by station. An open chain runs from the end `seam` (a point on it)
/// is nearer, or else the way that puts its ends nearer their partners. A closed chain runs round
/// the same way as the first (by their area vectors; the nearer pairing when they stand almost
/// at right angles), from `seam` when given, or else from the start that puts each station
/// closest to its partner: two rims whose seams sit apart would otherwise give a twisted blend.
fn pair_with(a: &[(Vec3, usize)], line: &ChainLine, seam: Option<Vec3>) -> Result<Vec<(Vec3, usize)>> {
    let count = a.len();
    let cost = |start: f64, backwards: bool| -> Result<f64> {
        Ok(line.stations(count, start, backwards)?.iter().zip(a).map(|((q, _), (p, _))| p.dist(*q)).sum())
    };
    // The least-cost start and way round among `starts` × `directions`.
    let best_of = |starts: &[f64], directions: &[bool]| -> Result<(f64, bool)> {
        let mut best = (0.0, false, f64::INFINITY);
        for &start in starts {
            for &backwards in directions {
                let c = cost(start, backwards)?;
                if c < best.2 {
                    best = (start, backwards, c);
                }
            }
        }
        Ok((best.0, best.1))
    };
    if !line.closed {
        let backwards = match seam {
            Some(p) => line.start_at_seam(p).1,
            None => cost(0.0, true)? < cost(0.0, false)?,
        };
        return line.stations(count, 0.0, backwards);
    }
    let total = line.length();
    let spacing = total / (count.max(2) - 1) as f64;
    let coarse: Vec<f64> = (0..4 * (count.max(2) - 1)).map(|k| k as f64 * spacing / 4.0).collect();
    // The way round: the same sense as the first loop, unless the two loops stand almost at right
    // angles (then the area vectors cannot say, and the nearer pairing does, with its start).
    let ring =
        |pts: &[(Vec3, usize)]| crate::plane_fit::loop_area(&pts.iter().take(pts.len().saturating_sub(1)).map(|(p, _)| *p).collect::<Vec<_>>());
    let (area_a, area_b) = (ring(a), ring(&line.stations(count, 0.0, false)?));
    let agree = area_a.dot(area_b);
    let (backwards, nearest_start) = if agree.abs() > 0.2 * area_a.len() * area_b.len() {
        (agree < 0.0, None)
    } else {
        let (start, backwards) = best_of(&coarse, &[false, true])?;
        (backwards, Some(start))
    };
    if let Some(p) = seam {
        return line.stations(count, line.start_at_seam(p).0, backwards);
    }
    // The best of starts four to a station spacing, narrowed down to a thousandth of a spacing
    // (the cost is smooth near its least).
    let mut start = match nearest_start {
        Some(start) => start,
        None => best_of(&coarse, &[backwards])?.0,
    };
    let (mut lo, mut hi) = (start - spacing / 4.0, start + spacing / 4.0);
    while hi - lo > spacing * 1e-3 {
        let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
        if cost(m1, backwards)? <= cost(m2, backwards)? {
            hi = m2;
        } else {
            lo = m1;
        }
        start = 0.5 * (lo + hi);
    }
    line.stations(count, start, backwards)
}

/// A blend surface between two picked edge chains of two bodies: the surfaces the blend leaves
/// and arrives at are the faces the chains bound (along each edge, the best-facing one), and each
/// side repeats that surface up to its requested continuity. Two closed chains blend station to
/// station from their seams — `seam` (a point on each chain) when given, otherwise the pairing
/// that keeps the blend from twisting; a closed chain does not blend to an open one. `align`
/// overrides the direction each side leaves in (points in world space); `crown` how full each side
/// is; `stitch` sews the blend to its two bodies where their boundaries meet. The reply's `gap`, if
/// any, is how far the blend's own boundary lay from the chains it was built on.
#[allow(clippy::too_many_arguments)]
pub fn blend_between(
    a: &Body,
    picks_a: &[Vec3],
    b: &Body,
    picks_b: &[Vec3],
    orders: [Continuity; 2],
    crown: [f64; 2],
    align: Option<[Vec3; 2]>,
    seam: Option<[Vec3; 2]>,
    samples: usize,
    stitch: bool,
) -> Result<(Body, f64)> {
    a.require_brep("blend")?;
    b.require_brep("blend")?;
    if !(5..=401).contains(&samples) {
        return Err(KernelError::Invalid("a blend needs 5 to 401 stations".into()));
    }
    if align.iter().chain(&seam).flatten().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid("`align` and `seam` points must be finite".into()));
    }
    let chain_a = chain(a, picks_a)?;
    let chain_b = chain(b, picks_b)?;
    let (line_a, line_b) = (ChainLine::of(a, &chain_a)?, ChainLine::of(b, &chain_b)?);
    let (record_a, record_b) = (a.record(), b.record());
    if line_a.closed != line_b.closed {
        return Err(KernelError::Invalid("a closed chain blends only to a closed chain, and an open one only to an open one".into()));
    }
    let (mid_a, mid_b) = (line_a.middle()?, line_b.middle()?);
    let (leave_a, leave_b) = match align {
        Some([p, q]) => (
            (p - mid_a).normalized().ok_or_else(|| KernelError::Invalid("`align` must not sit on the boundary".into()))?,
            (q - mid_b).normalized().ok_or_else(|| KernelError::Invalid("`align` must not sit on the boundary".into()))?,
        ),
        None => (
            (mid_b - mid_a).normalized().ok_or_else(|| KernelError::Invalid("the two boundaries coincide: there is nothing to blend".into()))?,
            (mid_a - mid_b).normalized().ok_or_else(|| KernelError::Invalid("the two boundaries coincide: there is nothing to blend".into()))?,
        ),
    };
    let faces_a = support_faces(record_a, &chain_a, leave_a, a.size())?;
    let faces_b = support_faces(record_b, &chain_b, leave_b, b.size())?;
    let (start_a, backwards_a) = seam.map_or((0.0, false), |[p, _]| line_a.start_at_seam(p));
    let stations_a = line_a.stations(samples, start_a, backwards_a)?;
    let stations_b = pair_with(&stations_a, &line_b, seam.map(|[_, q]| q))?;
    let frames_a = frames_on(record_a, &faces_a, &stations_a, &line_a, leave_a)?;
    let frames_b = frames_on(record_b, &faces_b, &stations_b, &line_b, leave_b)?;
    let made = bridge(&frames_a, &frames_b, orders, crown)?;
    let gap = boundary_gap(&made, &line_a, false)?.max(boundary_gap(&made, &line_b, true)?);
    // The blend's face is new to the operation over `[a, b]`.
    let blend = crate::surfaces::all_new(made, &[a, b])?;
    if !stitch {
        return Ok((blend, gap));
    }
    // Sewing up a boundary the blend only approximates: the stitch is given what the boundary
    // actually strays, so that a real gap between the bodies is still refused.
    let out = crate::surfaces::stitch(&[a, b, &blend], gap * 1.05 + 1e-9)?;
    Ok((out, gap))
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;
