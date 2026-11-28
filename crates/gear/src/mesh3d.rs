//! A pure-math check that the members of a 3D gear set mesh.
//!
//! A planned body's solid is known exactly from its steps: at a height in the body's frame a
//! prism, taper or loft is its outline turned, scaled or blended between sections; a coil is its
//! section carried up a helix; a revolve is a polygon turned about the axis. So a [`Solid`] can
//! say whether a point is inside it ([`Solid::contains`]) and give points on its surface with
//! their normals ([`Surface`]). Two bodies are compared by measuring every surface point of one
//! against the other's surface ([`mesh_stat`]): how deep the deepest one is inside (penetration),
//! how near the nearest one is outside (gap), and about how much volume they share. Each body
//! can be turned about its own axis ([`Solid::spin`]), so a pair can be stepped through its
//! motion ([`Pair::run`]): no penetration and a gap of about the backlash at every step is a set
//! that meshes; a gap that opens means the teeth are apart, not in mesh.
//!
//! The sweeps are modelled as the steps define them (a prism's twist linear in height, a
//! taper's by the logarithm of its scale, a loft of four or more sections cubic across them as
//! the kernel skins it, of two straight), so this proves the plan's geometry; what the kernel
//! makes of it is checked separately with `inspect.interference`.
//!
//! [`topology`] and [`check`] give the window's mesh verdict for the sets whose axes are not
//! parallel (`Plan::meshes3d`, through `mesh2d::check_mesh`): one tooth pitch, coarse samples.

use std::f64::consts::{PI, TAU};
use std::ops::Range;

use crate::axes::{cross, dot, sub};
use crate::geom::{Outline, Seg, V2, natural_spline};
use crate::mesh2d::{MeshResult, Verdict};
use crate::plan::{Body, Frame, Kind, Op, Plan, Spec, Step, taper_turn};

type P3 = [f64; 3];

fn arc_sweep(c: V2, a: V2, b: V2, ccw: bool) -> (f64, f64, f64) {
    let r = c.dist(a);
    let a0 = (a - c).angle();
    let mut sweep = (b - c).angle() - a0;
    if ccw {
        while sweep <= 0.0 {
            sweep += TAU;
        }
    } else {
        while sweep >= 0.0 {
            sweep -= TAU;
        }
    }
    (r, a0, sweep)
}

/// How many pieces each segment of `o` is cut into for points about `h` apart (a spline: each
/// of its spans).
pub fn seg_counts(o: &Outline, h: f64) -> Vec<usize> {
    let n = |len: f64| ((len / h).ceil() as usize).clamp(1, 4000);
    o.segs
        .iter()
        .map(|s| match s {
            Seg::Line(a, b) => n(a.dist(*b)),
            Seg::Arc { c, a, b, ccw } => {
                let (r, _, sweep) = arc_sweep(*c, *a, *b, *ccw);
                n(r * sweep.abs()).max((sweep.abs() / 0.05).ceil() as usize)
            }
            Seg::Spline(p) => n(p.windows(2).map(|w| w[0].dist(w[1])).fold(0.0, f64::max) * 1.05).max(4),
            Seg::Circle(_, r) => n(TAU * r).max(48),
        })
        .collect()
}

/// The outline as a closed polygon (the last point is not the first again), each segment cut
/// into its count of pieces: outlines of the same make sampled with the same counts give
/// points that correspond.
pub fn polygon(o: &Outline, counts: &[usize]) -> Vec<V2> {
    let mut out: Vec<V2> = Vec::new();
    for (s, n) in o.segs.iter().zip(counts) {
        let mut pts = Vec::new();
        match s {
            Seg::Line(a, b) => pts.extend((0..=*n).map(|i| a.lerp(*b, i as f64 / *n as f64))),
            Seg::Arc { c, a, b, ccw } => {
                let (r, a0, sweep) = arc_sweep(*c, *a, *b, *ccw);
                pts.extend((0..=*n).map(|i| *c + V2::polar(r, a0 + sweep * i as f64 / *n as f64)));
            }
            Seg::Spline(p) => natural_spline(p, *n, &mut pts),
            Seg::Circle(c, r) => pts.extend((0..=*n).map(|i| *c + V2::polar(*r, TAU * i as f64 / *n as f64))),
        }
        if !out.is_empty() && !pts.is_empty() {
            pts.remove(0);
        }
        out.extend(pts);
    }
    if out.len() > 1 && out[0].dist(out[out.len() - 1]) < 1e-9 {
        out.pop();
    }
    out
}

const BUCKETS: usize = 1440;

fn bucket_of(u: V2) -> usize {
    let mut f = u.angle();
    if f < 0.0 {
        f += TAU;
    }
    ((f / (TAU / BUCKETS as f64)).floor() as usize).min(BUCKETS - 1)
}

/// The angles an edge covers seen from the origin: (from, to), `to` ≥ `from`.
fn edge_span(a: V2, b: V2) -> (f64, f64) {
    let fa = a.angle();
    let d = crate::geom::wrap(b.angle() - fa);
    if d >= 0.0 { (fa, fa + d) } else { (fa + d, fa) }
}

fn fill_buckets(buckets: &mut [Vec<u32>], i: usize, lo: f64, hi: f64) {
    let w = TAU / BUCKETS as f64;
    let (i0, i1) = ((lo / w).floor() as i64 - 1, (hi / w).floor() as i64 + 1);
    if i1 - i0 >= BUCKETS as i64 {
        for b in buckets.iter_mut() {
            b.push(i as u32);
        }
        return;
    }
    for k in i0..=i1 {
        let idx = k.rem_euclid(BUCKETS as i64) as usize;
        if buckets[idx].last() != Some(&(i as u32)) {
            buckets[idx].push(i as u32);
        }
    }
}

/// Does the ray from `p` straight away from the origin cross the edge a→b?
fn crosses(a: V2, b: V2, u: V2, rho: f64) -> bool {
    let (sa, sb) = (u.cross(a), u.cross(b));
    if (sa > 0.0) != (sb > 0.0) {
        let t = sa / (sa - sb);
        (a + (b - a) * t).dot(u) > rho
    } else {
        false
    }
}

/// A closed polygon with its edges bucketed by angle about the origin, for fast inside tests.
#[derive(Clone)]
pub struct Poly {
    pub pts: Vec<V2>,
    buckets: Vec<Vec<u32>>,
    pub rmax: f64,
}

impl Poly {
    pub fn new(o: &Outline, h: f64) -> Poly {
        Poly::from_points(polygon(o, &seg_counts(o, h)))
    }
    pub fn from_points(mut pts: Vec<V2>) -> Poly {
        if pts.len() > 1 && pts[0].dist(pts[pts.len() - 1]) < 1e-12 {
            pts.pop();
        }
        let n = pts.len();
        let mut buckets = vec![Vec::new(); BUCKETS];
        let mut rmax = 0.0f64;
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            rmax = rmax.max(a.len());
            let (lo, hi) = edge_span(a, b);
            fill_buckets(&mut buckets, i, lo, hi);
        }
        Poly { pts, buckets, rmax }
    }
    /// Inside, by the crossings of the ray from `p` straight away from the origin.
    pub fn contains(&self, p: V2) -> bool {
        let rho = p.len();
        if rho > self.rmax {
            return false;
        }
        let u = if rho > 1e-12 { p * (1.0 / rho) } else { V2::new(1.0, 0.0) };
        let n = self.pts.len();
        let mut inside = false;
        for &i in &self.buckets[bucket_of(u)] {
            if crosses(self.pts[i as usize], self.pts[(i as usize + 1) % n], u, rho) {
                inside = !inside;
            }
        }
        inside
    }
}

/// Two corresponding polygons on planes z = `z0` and `z1`, joined straight point to point.
struct Slab {
    a: Vec<V2>,
    b: Vec<V2>,
    z0: f64,
    z1: f64,
    buckets: Vec<Vec<u32>>,
    rmax: f64,
}

impl Slab {
    fn new(a: Vec<V2>, b: Vec<V2>, z0: f64, z1: f64) -> Slab {
        let n = a.len();
        let mut buckets = vec![Vec::new(); BUCKETS];
        let mut rmax = 0.0f64;
        for i in 0..n {
            let j = (i + 1) % n;
            rmax = rmax.max(a[i].len()).max(b[i].len());
            let (lo, hi) = edge_span(a[i], a[j]);
            let (lo2, hi2) = edge_span(b[i], b[j]);
            // The second edge's span, on the same turn as the first's.
            let shift = crate::geom::wrap(lo2 - lo) - (lo2 - lo);
            fill_buckets(&mut buckets, i, lo.min(lo2 + shift), hi.max(hi2 + shift));
        }
        Slab { a, b, z0, z1, buckets, rmax }
    }
    fn contains(&self, p: V2, z: f64) -> bool {
        let rho = p.len();
        if rho > self.rmax {
            return false;
        }
        let t = if self.z1 > self.z0 { (z - self.z0) / (self.z1 - self.z0) } else { 0.0 };
        let u = if rho > 1e-12 { p * (1.0 / rho) } else { V2::new(1.0, 0.0) };
        let n = self.a.len();
        let at = |i: usize| self.a[i].lerp(self.b[i], t);
        let mut inside = false;
        for &i in &self.buckets[bucket_of(u)] {
            if crosses(at(i as usize), at((i as usize + 1) % n), u, rho) {
                inside = !inside;
            }
        }
        inside
    }
}

/// Inside a small polygon (any shape), by ray crossings along +x.
fn in_polygon(pts: &[V2], p: V2) -> bool {
    let n = pts.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// A polygon with points added along its edges so none is longer than `h`.
fn refined(pts: &[V2], h: f64) -> Vec<V2> {
    let n = pts.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let k = ((a.dist(b) / h).ceil() as usize).max(1);
        out.extend((0..k).map(|j| a.lerp(b, j as f64 / k as f64)));
    }
    out
}

/// The weights of the values at `xs` that give an interpolating spline's value at `x`: straight
/// between two, a parabola through three, the not-a-knot cubic through four or more (which a
/// cubic B-spline interpolating at the points with averaged knots is).
pub(crate) fn spline_weights(xs: &[f64], x: f64) -> Vec<f64> {
    let n = xs.len();
    match n {
        0 => Vec::new(),
        1 => vec![1.0],
        2 => {
            let t = (x - xs[0]) / (xs[1] - xs[0]);
            vec![1.0 - t, t]
        }
        3 => (0..3).map(|i| (0..3).filter(|&j| j != i).map(|j| (x - xs[j]) / (xs[i] - xs[j])).product()).collect(),
        _ => {
            let h: Vec<f64> = (0..n - 1).map(|i| xs[i + 1] - xs[i]).collect();
            let k = (0..n - 1).find(|&i| x <= xs[i + 1]).unwrap_or(n - 2);
            // The spline through the unit value at each point d: its second derivatives, all n
            // systems (one matrix, n right-hand sides) solved together. Each right-hand side gets
            // exactly the operations its own solve would, so the weights are the same numbers.
            let mut a = vec![vec![0.0; 2 * n]; n];
            a[0][0] = h[1];
            a[0][1] = -(h[0] + h[1]);
            a[0][2] = h[0];
            for i in 1..n - 1 {
                a[i][i - 1] = h[i - 1];
                a[i][i] = 2.0 * (h[i - 1] + h[i]);
                a[i][i + 1] = h[i];
                for d in 0..n {
                    let y = |j: usize| if j == d { 1.0 } else { 0.0 };
                    a[i][n + d] = 6.0 * ((y(i + 1) - y(i)) / h[i] - (y(i) - y(i - 1)) / h[i - 1]);
                }
            }
            a[n - 1][n - 3] = h[n - 2];
            a[n - 1][n - 2] = -(h[n - 3] + h[n - 2]);
            a[n - 1][n - 1] = h[n - 3];
            let ms = solve_many(a, n);
            (0..n)
                .map(|d| {
                    let y = |i: usize| if i == d { 1.0 } else { 0.0 };
                    let m = &ms[d];
                    let (t0, t1) = (xs[k + 1] - x, x - xs[k]);
                    let hk = h[k];
                    m[k] * t0.powi(3) / (6.0 * hk)
                        + m[k + 1] * t1.powi(3) / (6.0 * hk)
                        + (y(k) / hk - m[k] * hk / 6.0) * t0
                        + (y(k + 1) / hk - m[k + 1] * hk / 6.0) * t1
                })
                .collect()
        }
    }
}

/// Gaussian elimination with partial pivoting of `r` systems with one matrix: `a` is
/// n × (n + r), the matrix then the right-hand sides; the solutions, one per right-hand side.
// The elimination indexes two rows of one matrix at once, which reads plainest as indices.
#[allow(clippy::needless_range_loop)]
fn solve_many(mut a: Vec<Vec<f64>>, r: usize) -> Vec<Vec<f64>> {
    let n = a.len();
    for c in 0..n {
        let p = (c..n).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs())).unwrap_or(c);
        a.swap(c, p);
        let d = a[c][c];
        if d.abs() < 1e-300 {
            continue;
        }
        for row in 0..n {
            if row != c {
                let f = a[row][c] / d;
                if f != 0.0 {
                    for k in c..n + r {
                        a[row][k] -= f * a[c][k];
                    }
                }
            }
        }
    }
    (0..r).map(|d| (0..n).map(|i| if a[i][i].abs() < 1e-300 { 0.0 } else { a[i][n + d] / a[i][i] }).collect()).collect()
}

/// Corresponding polygons at `zs`, with `sub` more between each two on the spline the kernel's
/// loft skins through them.
fn across(polys: &[Vec<V2>], zs: &[f64], sub: usize) -> (Vec<Vec<V2>>, Vec<f64>) {
    if polys.len() < 3 {
        return (polys.to_vec(), zs.to_vec());
    }
    let (mut out, mut at) = (Vec::new(), Vec::new());
    for j in 0..zs.len() - 1 {
        for s in 0..sub {
            let z = zs[j] + (zs[j + 1] - zs[j]) * s as f64 / sub as f64;
            if s == 0 {
                out.push(polys[j].clone());
            } else {
                let w = spline_weights(zs, z);
                out.push((0..polys[0].len()).map(|i| w.iter().zip(polys).fold(V2::ZERO, |acc, (wk, p)| acc + p[i] * *wk)).collect());
            }
            at.push(z);
        }
    }
    out.push(polys[polys.len() - 1].clone());
    at.push(zs[zs.len() - 1]);
    (out, at)
}

enum Part {
    Prism {
        outer: Vec<Poly>,
        holes: Vec<Poly>,
        z0: f64,
        height: f64,
        twist: f64,
    },
    Taper {
        poly: Poly,
        z0: f64,
        height: f64,
        twist: f64,
        scale: f64,
    },
    /// Inside the cylinder of radius `rim` and outside the taper.
    RingTaper {
        poly: Poly,
        rim: f64,
        z0: f64,
        height: f64,
        twist: f64,
        scale: f64,
    },
    Loft {
        slabs: Vec<Slab>,
    },
    Revolve {
        profile: Vec<V2>,
    },
    Coil {
        section: Vec<V2>,
        pitch: f64,
        turns: f64,
        left: bool,
        zlo: f64,
        zhi: f64,
    },
    /// A part in a frame of its own, given in the body's.
    Placed {
        frame: Frame,
        inner: Box<Part>,
    },
    /// A solid of its own made of parts, each combined with the ones before by its op.
    Group {
        parts: Vec<(Part, Op)>,
    },
}

/// A step as a part, its op, and the cylinder about the body's axis that holds it (radius,
/// least and most z).
fn make_part(st: &Step, h: f64) -> (Part, Op, (f64, f64, f64)) {
    match st {
        Step::Prism { region, z0, height, twist, op } => {
            let polys = |v: &Vec<Outline>| v.iter().map(|o| Poly::new(o, h)).collect::<Vec<_>>();
            let (outer, holes) = (polys(&region.outer), polys(&region.holes));
            let r = outer.iter().fold(0.0f64, |a, o| a.max(o.rmax));
            (Part::Prism { outer, holes, z0: *z0, height: *height, twist: *twist }, *op, (r, *z0, z0 + height))
        }
        Step::Taper { outline, z0, height, twist, scale, op } => {
            let poly = Poly::new(outline, h);
            let r = poly.rmax * scale.max(1.0);
            (Part::Taper { poly, z0: *z0, height: *height, twist: *twist, scale: *scale }, *op, (r, *z0, z0 + height))
        }
        Step::RingTaper { outline, rim, z0, height, twist, scale, op } => {
            let poly = Poly::new(outline, h);
            (Part::RingTaper { poly, rim: *rim, z0: *z0, height: *height, twist: *twist, scale: *scale }, *op, (*rim, *z0, z0 + height))
        }
        Step::Loft { sections, op } => {
            // The same cut of every section, fine enough for the largest.
            let mut counts = seg_counts(&sections[0].0, h);
            for (o, _) in sections {
                assert_eq!(o.segs.len(), counts.len(), "loft sections must be made of the same segments");
                for (c, k) in counts.iter_mut().zip(seg_counts(o, h)) {
                    *c = (*c).max(k);
                }
            }
            let polys: Vec<Vec<V2>> = sections.iter().map(|(o, _)| polygon(o, &counts)).collect();
            assert!(polys.iter().all(|p| p.len() == polys[0].len()), "loft sections must correspond point for point");
            // The kernel skins a loft of four or more sections cubic across them (three:
            // quadratic), so the solid between two sections is that curve's, here followed by
            // short straight slabs.
            let zs: Vec<f64> = sections.iter().map(|s| s.1).collect();
            // (Many close sections, as a hypoid pinion's 41, need fewer slabs between; a hypoid
            // ring gear's 19 on its flat cone move far between sections and need four: with two
            // the model's flanks strayed 0.06 mm and its heel face showed a false overlap.)
            let (polys, zs) = across(&polys, &zs, if zs.len() >= 30 { 2 } else { 4 });
            let slabs: Vec<Slab> = (0..zs.len() - 1).map(|j| Slab::new(polys[j].clone(), polys[j + 1].clone(), zs[j], zs[j + 1])).collect();
            let r = slabs.iter().fold(0.0f64, |a, s| a.max(s.rmax));
            (Part::Loft { slabs }, *op, (r, sections[0].1, sections[sections.len() - 1].1))
        }
        Step::Revolve { profile, op } => {
            let r = profile.iter().fold(0.0f64, |a, q| a.max(q.x));
            let (lo, hi) = profile.iter().fold((f64::MAX, f64::MIN), |a, q| (a.0.min(q.y), a.1.max(q.y)));
            (Part::Revolve { profile: profile.clone() }, *op, (r, lo, hi))
        }
        Step::Coil { section, pitch, height, left, op, .. } => {
            let zlo = section.iter().map(|q| q.y).fold(f64::MAX, f64::min);
            let zhi = section.iter().map(|q| q.y).fold(f64::MIN, f64::max);
            let r = section.iter().fold(0.0f64, |a, q| a.max(q.x));
            (Part::Coil { section: section.clone(), pitch: *pitch, turns: height / pitch, left: *left, zlo, zhi }, *op, (r, zlo, zhi + height))
        }
        Step::Placed { frame, step } => {
            let (inner, op, (r, lo, hi)) = make_part(step, h);
            // The inner part's cylinder, by points round its rims, in the body's coordinates.
            let (mut rr, mut zlo, mut zhi) = (0.0f64, f64::MAX, f64::MIN);
            for z in [lo, hi] {
                for k in 0..32 {
                    let c = V2::polar(r, TAU * f64::from(k) / 32.0);
                    let q = frame.world([c.x, c.y, z]);
                    rr = rr.max(q[0].hypot(q[1]));
                    zlo = zlo.min(q[2]);
                    zhi = zhi.max(q[2]);
                }
            }
            let slack = r * (1.0 - (PI / 32.0).cos()) / (PI / 32.0).cos();
            (Part::Placed { frame: *frame, inner: Box::new(inner) }, op, (rr + slack, zlo - slack, zhi + slack))
        }
        Step::Group { steps, op } => {
            let mut parts = Vec::with_capacity(steps.len());
            let (mut r, mut lo, mut hi) = (0.0f64, f64::MAX, f64::MIN);
            for st in steps {
                let (part, o, (pr, plo, phi)) = make_part(st, h);
                if matches!(o, Op::New | Op::Join) {
                    (r, lo, hi) = (r.max(pr), lo.min(plo), hi.max(phi));
                }
                parts.push((part, o));
            }
            (Part::Group { parts }, *op, (r, lo, hi))
        }
    }
}

impl Part {
    fn contains(&self, p: P3) -> bool {
        let q = V2::new(p[0], p[1]);
        match self {
            Part::Prism { outer, holes, z0, height, twist } => {
                let t = (p[2] - z0) / height;
                if !(0.0..=1.0).contains(&t) {
                    return false;
                }
                let q = q.rot(-twist * t);
                outer.iter().any(|o| o.contains(q)) && !holes.iter().any(|o| o.contains(q))
            }
            Part::Taper { poly, z0, height, twist, scale } => {
                let t = (p[2] - z0) / height;
                if !(0.0..=1.0).contains(&t) {
                    return false;
                }
                let s = 1.0 + (scale - 1.0) * t;
                poly.contains(q.rot(-taper_turn(*twist, *scale, s)) * (1.0 / s))
            }
            Part::RingTaper { poly, rim, z0, height, twist, scale } => {
                let t = (p[2] - z0) / height;
                if !(0.0..=1.0).contains(&t) || q.len() > *rim {
                    return false;
                }
                let s = 1.0 + (scale - 1.0) * t;
                !poly.contains(q.rot(-taper_turn(*twist, *scale, s)) * (1.0 / s))
            }
            Part::Loft { slabs } => {
                // Slabs that run up z in order: the first whose top is at or above the point.
                if slabs.first().zip(slabs.last()).is_some_and(|(a, b)| a.z0 <= b.z1) {
                    let k = slabs.partition_point(|s| s.z1 < p[2]);
                    slabs.get(k).is_some_and(|s| p[2] >= s.z0 && s.contains(q, p[2]))
                } else {
                    slabs.iter().find(|s| p[2] >= s.z0 && p[2] <= s.z1).is_some_and(|s| s.contains(q, p[2]))
                }
            }
            Part::Revolve { profile } => in_polygon(profile, V2::new(q.len(), p[2])),
            Part::Coil { section, pitch, turns, left, zlo, zhi } => {
                let r = q.len();
                let mut th = q.angle();
                if *left {
                    th = -th;
                }
                if th < 0.0 {
                    th += TAU;
                }
                // The section at angle th + 2πn sits pitch·(th/2π + n) up the axis.
                let base = p[2] - pitch * th / TAU;
                let (n0, n1) = (((base - zhi) / pitch).floor() as i64, ((base - zlo) / pitch).ceil() as i64);
                for n in n0.max(0)..=n1 {
                    if th / TAU + n as f64 > *turns {
                        break;
                    }
                    if in_polygon(section, V2::new(r, base - pitch * n as f64)) {
                        return true;
                    }
                }
                false
            }
            Part::Placed { frame, inner } => {
                let d = sub(p, frame.origin);
                inner.contains([dot(d, frame.x), dot(d, frame.y()), dot(d, frame.z)])
            }
            Part::Group { parts } => {
                let mut inside = false;
                for (part, op) in parts {
                    match op {
                        Op::New | Op::Join => inside = inside || part.contains(p),
                        Op::Cut => inside = inside && !part.contains(p),
                        Op::Intersect => inside = inside && part.contains(p),
                    }
                }
                inside
            }
        }
    }
    /// Whether a point of this part's own surface ([`Part::sheets`]) is kept by the part: a
    /// group's sheets are its pieces' whole surfaces, of which its own trims keep only some.
    /// Every other part keeps all of its surface.
    fn keeps(&self, p: &P3) -> bool {
        match self {
            Part::Group { parts } => parts.iter().all(|(part, op)| match op {
                Op::Intersect => part.contains(*p),
                Op::Cut => !part.contains(*p),
                Op::New | Op::Join => true,
            }),
            _ => true,
        }
    }
    /// [`Part::sheets`] with the sheets whose every row is one outline turned and scaled (a
    /// prism's, a taper's) kept as that outline and each row's turn, scale and z: the same rows,
    /// not made. The other sheets as `sheets` makes them; with `first_revolved`, a revolved sheet
    /// (every row the same profile turned: the same radius at each z in every row) only its
    /// first row.
    #[allow(clippy::type_complexity)]
    fn turned_rows(&self, h: f64, hz: f64, first_revolved: bool) -> (Vec<(Vec<V2>, Vec<(f64, f64, f64)>)>, Vec<Vec<Vec<P3>>>) {
        let layers = |len: f64| ((len.abs() / hz).ceil() as usize).clamp(1, 20000);
        match self {
            Part::Prism { outer, holes, z0, height, twist } => {
                let turned = outer
                    .iter()
                    .chain(holes)
                    .map(|poly| {
                        let n = layers(height.hypot(poly.rmax * twist.abs()));
                        let rows = (0..=n)
                            .map(|j| {
                                let t = j as f64 / n as f64;
                                (twist * t, 1.0, z0 + height * t)
                            })
                            .collect();
                        (poly.pts.clone(), rows)
                    })
                    .collect();
                (turned, Vec::new())
            }
            Part::Taper { poly, z0, height, twist, scale } => {
                let n = layers(height.hypot(poly.rmax * (1.0 - scale).abs() * scale.max(1.0)).hypot(poly.rmax * twist.abs()));
                let rows = (0..=n)
                    .map(|j| {
                        let t = j as f64 / n as f64;
                        let s = 1.0 + (scale - 1.0) * t;
                        (taper_turn(*twist, *scale, s), s, z0 + height * t)
                    })
                    .collect();
                (vec![(poly.pts.clone(), rows)], Vec::new())
            }
            Part::RingTaper { poly, rim, z0, height, twist, scale } => {
                let teeth = Part::Taper { poly: poly.clone(), z0: *z0, height: *height, twist: *twist, scale: *scale };
                let wall =
                    Part::Revolve { profile: vec![V2::new(0.0, *z0), V2::new(*rim, *z0), V2::new(*rim, z0 + height), V2::new(0.0, z0 + height)] };
                (teeth.turned_rows(h, hz, false).0, wall.turned_rows(h, hz, first_revolved).1)
            }
            Part::Revolve { profile } if first_revolved => {
                let pts = refined(profile, h);
                (Vec::new(), vec![vec![pts.iter().map(|q| [q.x, 0.0, q.y]).collect()]])
            }
            _ => (Vec::new(), self.sheets(h, hz, (f64::MIN, f64::MAX))),
        }
    }
    /// The part's side surface as rows of corresponding points (each row a closed loop), the
    /// rows about `hz` apart: one list of rows per sheet. A coil's rows are only those that
    /// reach into the `band` of heights.
    fn sheets(&self, h: f64, hz: f64, band: (f64, f64)) -> Vec<Vec<Vec<P3>>> {
        let layers = |len: f64| ((len.abs() / hz).ceil() as usize).clamp(1, 20000);
        let mut out = Vec::new();
        match self {
            Part::Prism { outer, holes, z0, height, twist } => {
                for poly in outer.iter().chain(holes) {
                    let turned = poly.rmax * twist.abs();
                    let n = layers(height.hypot(turned));
                    out.push(
                        (0..=n)
                            .map(|j| {
                                let t = j as f64 / n as f64;
                                poly.pts.iter().map(|q| q.rot(twist * t)).map(|q| [q.x, q.y, z0 + height * t]).collect()
                            })
                            .collect(),
                    );
                }
            }
            Part::RingTaper { poly, rim, z0, height, twist, scale } => {
                let teeth = Part::Taper { poly: poly.clone(), z0: *z0, height: *height, twist: *twist, scale: *scale };
                out.extend(teeth.sheets(h, hz, band));
                let wall =
                    Part::Revolve { profile: vec![V2::new(0.0, *z0), V2::new(*rim, *z0), V2::new(*rim, z0 + height), V2::new(0.0, z0 + height)] };
                out.extend(wall.sheets(h, hz, band));
            }
            Part::Taper { poly, z0, height, twist, scale } => {
                // Rows about `hz` apart along the surface: up the axis, out along the taper's
                // lines through the apex (long on a flat cone, whose teeth run nearly square to
                // the axis) and round with its turn.
                let n = layers(height.hypot(poly.rmax * (1.0 - scale).abs() * scale.max(1.0)).hypot(poly.rmax * twist.abs()));
                out.push(
                    (0..=n)
                        .map(|j| {
                            let t = j as f64 / n as f64;
                            let s = 1.0 + (scale - 1.0) * t;
                            let turn = taper_turn(*twist, *scale, s);
                            poly.pts.iter().map(|q| q.rot(turn) * s).map(|q| [q.x, q.y, z0 + height * t]).collect()
                        })
                        .collect(),
                );
            }
            Part::Loft { slabs } => {
                for s in slabs {
                    let n = layers(s.z1 - s.z0);
                    out.push(
                        (0..=n)
                            .map(|j| {
                                let t = j as f64 / n as f64;
                                s.a.iter().zip(&s.b).map(|(a, b)| a.lerp(*b, t)).map(|q| [q.x, q.y, s.z0 + (s.z1 - s.z0) * t]).collect()
                            })
                            .collect(),
                    );
                }
            }
            Part::Revolve { profile } => {
                let pts = refined(profile, h);
                let rmax = pts.iter().map(|q| q.x).fold(0.0, f64::max);
                let n = layers(TAU * rmax).max(24);
                out.push(
                    (0..=n)
                        .map(|j| pts.iter().map(|q| V2::polar(q.x, TAU * j as f64 / n as f64)).zip(&pts).map(|(c, q)| [c.x, c.y, q.y]).collect())
                        .collect(),
                );
            }
            Part::Coil { section, pitch, turns, left, zlo, zhi } => {
                let pts = refined(section, h);
                let rmax = pts.iter().map(|q| q.x).fold(0.0, f64::max);
                let n = layers(turns * (TAU * rmax).hypot(*pitch)).max(24);
                let hand = if *left { -1.0 } else { 1.0 };
                // Only the rows whose section reaches into the band.
                let row = |z: f64| (z / (pitch * turns) * n as f64).clamp(0.0, n as f64);
                let (j0, j1) = (row(band.0 - zhi).floor() as usize, row(band.1 - zlo).ceil() as usize);
                out.push(
                    (j0..=j1)
                        .map(|j| {
                            let th = TAU * turns * j as f64 / n as f64;
                            pts.iter().map(|q| (V2::polar(q.x, hand * th), q.y + pitch * th / TAU)).map(|(c, z)| [c.x, c.y, z]).collect()
                        })
                        .collect(),
                );
            }
            Part::Placed { frame, inner } => {
                for rows in inner.sheets(h, hz, (f64::MIN, f64::MAX)) {
                    out.push(rows.into_iter().map(|row| row.into_iter().map(|q| frame.world(q)).collect()).collect());
                }
            }
            // The pieces that add material, whole: [`Part::keeps`] says which points stay.
            Part::Group { parts } => {
                for (part, op) in parts {
                    if matches!(op, Op::New | Op::Join) {
                        out.extend(part.sheets(h, hz, band));
                    }
                }
            }
        }
        out
    }
}

/// A planned body as a solid to test points against, with a spin about its own axis. (Clones
/// share the parts: a clone is a cheap copy to turn differently, as the steps checked at once
/// need.)
#[derive(Clone)]
pub struct Solid {
    pub frame: Frame,
    parts: std::sync::Arc<Vec<(Part, Op)>>,
    /// How far the body is turned about its frame's z axis (radians, counter-clockwise).
    pub spin: f64,
    /// A cylinder about the axis that holds the body: radius and z range (frame coordinates).
    pub radius: f64,
    pub z: (f64, f64),
    h: f64,
}

impl Solid {
    /// The body's solid, its curved outlines as polygons with points about `h` apart.
    pub fn new(body: &Body, h: f64) -> Solid {
        let mut parts = Vec::new();
        let (mut radius, mut zr) = (0.0f64, (f64::MAX, f64::MIN));
        for st in &body.steps {
            let (part, op, (r, lo, hi)) = make_part(st, h);
            if matches!(op, Op::New | Op::Join) {
                radius = radius.max(r);
                zr = (zr.0.min(lo), zr.1.max(hi));
            }
            parts.push((part, op));
        }
        Solid { frame: body.frame, parts: std::sync::Arc::new(parts), spin: 0.0, radius, z: zr, h }
    }
    /// A world point in the body's own coordinates (its spin undone).
    pub fn local(&self, p: P3) -> P3 {
        let f = &self.frame;
        let d = sub(p, f.origin);
        let q = V2::new(dot(d, f.x), dot(d, f.y())).rot(-self.spin);
        [q.x, q.y, dot(d, f.z)]
    }
    /// A point of the body (its own coordinates) in the world, the body turned by its spin.
    pub fn world(&self, p: P3) -> P3 {
        let q = V2::new(p[0], p[1]).rot(self.spin);
        self.frame.world([q.x, q.y, p[2]])
    }
    fn world_dir(&self, n: P3) -> P3 {
        let q = V2::new(n[0], n[1]).rot(self.spin);
        let (x, y, z) = (self.frame.x, self.frame.y(), self.frame.z);
        [0, 1, 2].map(|i| q.x * x[i] + q.y * y[i] + n[2] * z[i])
    }
    fn contains_local(&self, l: P3) -> bool {
        if l[2] < self.z.0 || l[2] > self.z.1 || l[0] * l[0] + l[1] * l[1] > self.radius * self.radius {
            return false;
        }
        let mut inside = false;
        for (part, op) in self.parts.iter() {
            match op {
                Op::New | Op::Join => inside = inside || part.contains(l),
                Op::Cut => inside = inside && !part.contains(l),
                Op::Intersect => inside = inside && part.contains(l),
            }
        }
        inside
    }
    /// Points on the side surfaces of the parts that add material (the first part and the
    /// joins), in the body's own coordinates (unturned): round each outline about `h` apart (the
    /// solid's own spacing) and along each sweep about `hz` apart. Whatever the body sweeps when it
    /// turns about its axis lies within these points' radii, row by row.
    ///
    /// A body trimmed by a part it is intersected with (a flat bevel's back and front cones)
    /// keeps only the points inside every such part, and adds the trimming part's own points
    /// that lie in the material: the trimmed body's surface, not the untrimmed one's. A trim
    /// takes away only from what was made before it, as the body is built: a hub or a shaft
    /// joined after the trim keeps all its points.
    pub fn material_points(&self, hz: f64) -> Vec<P3> {
        let mut out = Vec::new();
        let trims: Vec<usize> = self.parts.iter().enumerate().filter(|(_, (_, op))| *op == Op::Intersect).map(|(i, _)| i).collect();
        // A point of part `own` stays if every trim after it holds it.
        let kept = |p: &P3, own: usize| trims.iter().all(|&t| t <= own || self.parts[t].0.contains(*p));
        // A trim's own surface is material where something made before it was.
        let made_before = |p: &P3, trim: usize| self.parts[..trim].iter().any(|(q, o)| matches!(o, Op::New | Op::Join) && q.contains(*p));
        for (i, (part, op)) in self.parts.iter().enumerate() {
            let adds = matches!(op, Op::New | Op::Join);
            if !adds && *op != Op::Intersect {
                continue;
            }
            for sheet in part.sheets(self.h, hz, (f64::MIN, f64::MAX)) {
                // A trim's rows go round the axis: a few dozen of them show how far out its
                // faces carry material (testing every one against the teeth is slow).
                let stride = if adds { 1 } else { (sheet.len() / 96).max(1) };
                for row in sheet.into_iter().step_by(stride) {
                    if trims.is_empty() {
                        out.extend(row.into_iter().filter(|p| part.keeps(p)));
                        continue;
                    }
                    out.extend(row.into_iter().filter(|p| part.keeps(p) && kept(p, i) && (adds || made_before(p, i))));
                }
            }
        }
        out
    }
    /// Of [`Solid::material_points`], at each z (its own coordinates) the point farthest from the
    /// axis, by z: what an envelope about the axis reads of them. The same points, found without
    /// testing the rest of a row against the trims: a row's points are tried from the farthest
    /// in, and a row at one z stops at the first the trims keep.
    pub fn farthest_per_row(&self, hz: f64) -> Vec<P3> {
        let trims: Vec<usize> = self.parts.iter().enumerate().filter(|(_, (_, op))| *op == Op::Intersect).map(|(i, _)| i).collect();
        let kept = |p: &P3, own: usize| trims.iter().all(|&t| t <= own || self.parts[t].0.contains(*p));
        let made_before = |p: &P3, trim: usize| self.parts[..trim].iter().any(|(q, o)| matches!(o, Op::New | Op::Join) && q.contains(*p));
        let reach = |p: &P3| p[0].hypot(p[1]);
        let mut best: std::collections::HashMap<u64, P3> = std::collections::HashMap::new();
        let mut take = |p: P3| {
            let row = best.entry(p[2].to_bits()).or_insert(p);
            if reach(&p) > reach(row) {
                *row = p;
            }
        };
        for (i, (part, op)) in self.parts.iter().enumerate() {
            let adds = matches!(op, Op::New | Op::Join);
            if !adds && *op != Op::Intersect {
                continue;
            }
            // A revolved sheet adding material with no trim after it: its first row holds every
            // radius.
            let (turned, rest) = part.turned_rows(self.h, hz, adds && trims.iter().all(|&t| t <= i));
            for (outline, rows) in turned {
                // Every row is the outline turned and scaled: the same points farthest out.
                let stride = if adds { 1 } else { (rows.len() / 96).max(1) };
                let mut order: Vec<usize> = (0..outline.len()).collect();
                order.sort_by(|a, b| outline[*b].len().total_cmp(&outline[*a].len()));
                for &(turn, scale, z) in rows.iter().step_by(stride) {
                    for &k in &order {
                        let q = outline[k].rot(turn) * scale;
                        let p = [q.x, q.y, z];
                        if part.keeps(&p) && (trims.is_empty() || (kept(&p, i) && (adds || made_before(&p, i)))) {
                            take(p);
                            break;
                        }
                    }
                }
            }
            for sheet in rest {
                let stride = if adds { 1 } else { (sheet.len() / 96).max(1) };
                for row in sheet.into_iter().step_by(stride) {
                    if trims.is_empty() {
                        row.into_iter().filter(|p| part.keeps(p)).for_each(&mut take);
                        continue;
                    }
                    // Farthest first (ties in the row's order); each z of the row done at its
                    // first point kept.
                    let mut order: Vec<usize> = (0..row.len()).collect();
                    order.sort_by(|a, b| reach(&row[*b]).total_cmp(&reach(&row[*a])));
                    let one_z = row.iter().all(|p| p[2].to_bits() == row[0][2].to_bits());
                    let mut done: std::collections::HashSet<u64> = std::collections::HashSet::new();
                    for k in order {
                        let p = row[k];
                        let key = p[2].to_bits();
                        if done.contains(&key) {
                            continue;
                        }
                        if part.keeps(&p) && kept(&p, i) && (adds || made_before(&p, i)) {
                            take(p);
                            if one_z {
                                break;
                            }
                            done.insert(key);
                        }
                    }
                }
            }
        }
        let mut out: Vec<P3> = best.into_values().collect();
        out.sort_by(|a, b| a[2].total_cmp(&b[2]));
        out
    }
    /// Is the world point inside the body?
    pub fn contains(&self, p: P3) -> bool {
        self.contains_local(self.local(p))
    }
    /// Is the world point within `margin` of the cylinder that holds the body?
    fn near(&self, p: P3, margin: f64) -> bool {
        let l = self.local(p);
        l[2] >= self.z.0 - margin && l[2] <= self.z.1 + margin && l[0] * l[0] + l[1] * l[1] <= (self.radius + margin).powi(2)
    }
    /// The world box that holds the body's bounding cylinder.
    pub fn bounds(&self) -> (P3, P3) {
        let f = &self.frame;
        let (mut lo, mut hi) = ([0.0; 3], [0.0; 3]);
        for i in 0..3 {
            let side = self.radius * (1.0 - f.z[i] * f.z[i]).max(0.0).sqrt();
            let (a, b) = (f.origin[i] + f.z[i] * self.z.0, f.origin[i] + f.z[i] * self.z.1);
            lo[i] = a.min(b) - side;
            hi[i] = a.max(b) + side;
        }
        (lo, hi)
    }
    /// The band of this body (least radius, z range, its own coordinates) that can come within
    /// `margin` of `other`'s bounding cylinder however either is turned.
    /// Also the arc of angles about this body's own axis (unturned: its middle and half its
    /// width) the band lies in, when that is less than a full turn.
    fn reach_of(&self, other: &Solid, margin: f64) -> (f64, f64, f64, Option<(f64, f64)>) {
        let (mut rmin, mut zlo, mut zhi) = (f64::MAX, f64::MAX, f64::MIN);
        let mut angles = Vec::with_capacity(25 * 96 * 4);
        let rr = other.radius + margin;
        for k in 0..=24 {
            let z = other.z.0 - margin + (other.z.1 - other.z.0 + 2.0 * margin) * f64::from(k) / 24.0;
            for i in 0..96 {
                for f in [1.0, 0.66, 0.33, 0.0] {
                    let c = V2::polar(rr * f, TAU * f64::from(i) / 96.0);
                    let l = self.local(other.frame.world([c.x, c.y, z]));
                    rmin = rmin.min(l[0].hypot(l[1]));
                    zlo = zlo.min(l[2]);
                    zhi = zhi.max(l[2]);
                    angles.push(l[1].atan2(l[0]) + self.spin);
                }
            }
        }
        // The probe points are a grid: allow for what lies between them.
        let slack = 0.05 * rr + (other.z.1 - other.z.0) / 24.0;
        let rmin = (rmin - slack).max(0.0);
        // The arc: the whole turn less the widest gap between the probes' angles, widened by
        // the angle the grid's slack reaches at the least radius.
        for a in angles.iter_mut() {
            *a = a.rem_euclid(TAU);
        }
        angles.sort_by(f64::total_cmp);
        let n = angles.len();
        let (mut gap, mut from) = (angles[0] + TAU - angles[n - 1], angles[n - 1]);
        for w in angles.windows(2) {
            if w[1] - w[0] > gap {
                gap = w[1] - w[0];
                from = w[0];
            }
        }
        let widen = if rmin > 1e-9 { (2.0 * slack / rmin).min(PI) } else { PI };
        let half = 0.5 * (TAU - gap) + widen;
        let arc = if half < PI { Some((from + gap + 0.5 * (TAU - gap), half)) } else { None };
        (rmin, zlo - slack, zhi + slack, arc)
    }
}

/// A point on a body's surface (the body's own coordinates), the outward normal there, and
/// the area it stands for.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub p: P3,
    pub n: P3,
    /// Half the patch's two sides: the sample stands for the parallelogram p ± hu ± hv.
    pub hu: P3,
    pub hv: P3,
    pub area: f64,
}

const SECTORS: usize = 90;

/// The part of a body's surface that can meet another body, as samples grouped by their angle
/// about the axis.
pub struct Surface {
    pub samples: Vec<Sample>,
    sectors: Vec<Range<usize>>,
    /// About how far a sample is from the corners of the patch it stands for.
    reach: f64,
}

impl Surface {
    /// Samples of `solid`'s surface where it can meet `other`: about `h` apart round the
    /// outlines and `hz` apart along the sweeps. With `spins` (the least and most `solid` will
    /// be turned) only the arc of it that can come round to face `other` is sampled.
    pub fn of(solid: &Solid, other: &Solid, hz: f64, spins: Option<(f64, f64)>) -> Surface {
        let h = solid.h;
        let (rmin, zlo, zhi, arc) = solid.reach_of(other, 1.0);
        // The arc each sample's own angle must be in, in the body's own coordinates.
        let window = match (arc, spins) {
            (Some((mid, half)), Some((lo, hi))) if half + 0.5 * (hi - lo).abs() < PI => Some((mid - 0.5 * (lo + hi), half + 0.5 * (hi - lo).abs())),
            _ => None,
        };
        // Which rings about the axis (a grid of radius and height) come within reach of `other`
        // at any angle in the arc: only samples on those are kept.
        let (nr, nz, na) = (32usize, 64usize, 48usize);
        let rtop = solid.radius.max(rmin + 1e-9);
        let (dr, dz) = ((rtop - rmin) / nr as f64, (zhi - zlo).max(1e-9) / nz as f64);
        let (amid, ahalf) = arc.unwrap_or((0.0, PI));
        let da = 2.0 * ahalf / na as f64;
        let mut ring = vec![false; nr * nz];
        let (mut blo, mut bhi) = (f64::MAX, f64::MIN);
        for iz in 0..nz {
            let z = zlo + (iz as f64 + 0.5) * dz;
            for ir in 0..nr {
                let r = rmin + (ir as f64 + 0.5) * dr;
                let margin = 1.0 + 0.5 * dr.hypot(dz) + r * da;
                if (0..=na).any(|k| {
                    let q = V2::polar(r, amid - ahalf + da * k as f64);
                    other.near(solid.frame.world([q.x, q.y, z]), margin)
                }) {
                    ring[iz * nr + ir] = true;
                    blo = blo.min(z - dz);
                    bhi = bhi.max(z + dz);
                }
            }
        }
        let reachable = |p: P3| {
            let (ir, iz) = (((p[0].hypot(p[1]) - rmin) / dr).floor(), ((p[2] - zlo) / dz).floor());
            ir >= 0.0 && iz >= 0.0 && (ir as usize) < nr && (iz as usize) < nz && ring[iz as usize * nr + ir as usize] || ir >= nr as f64
        };
        let eps = 0.02 * h;
        let mut samples = Vec::new();
        for (part, op) in solid.parts.iter() {
            // A trim's faces (a bevel's back and front cones) are the body's ends, where its
            // teeth stop: like a taper's flat ends they aren't sampled. (Their patches would
            // straddle the teeth's edges and read as touching the mate's flank where it passes
            // a backlash away; what the trim leaves of the teeth is sampled on the teeth's own
            // sheets.)
            if *op == Op::Intersect {
                continue;
            }
            for rows in part.sheets(h, hz, (blo, bhi)) {
                for w in rows.windows(2) {
                    let (r0, r1) = (&w[0], &w[1]);
                    let n = r0.len();
                    for i in 0..n {
                        let j = (i + 1) % n;
                        let (a, b, c, d) = (r0[i], r0[j], r1[i], r1[j]);
                        let p = [0, 1, 2].map(|k| 0.25 * (a[k] + b[k] + c[k] + d[k]));
                        if p[2] < zlo
                            || p[2] > zhi
                            || p[0].hypot(p[1]) < rmin
                            || window.is_some_and(|(mid, half)| crate::geom::wrap(p[1].atan2(p[0]) - mid).abs() > half)
                            || !reachable(p)
                        {
                            continue;
                        }
                        let du = [0, 1, 2].map(|k| 0.5 * (b[k] + d[k] - a[k] - c[k]));
                        let dv = [0, 1, 2].map(|k| 0.5 * (c[k] + d[k] - a[k] - b[k]));
                        let nn = cross(du, dv);
                        let area = dot(nn, nn).sqrt();
                        if area < 1e-14 {
                            continue;
                        }
                        let nn = nn.map(|v| v / area);
                        // On the finished body's boundary, and which way is out.
                        let on = |p: P3| {
                            let inner = solid.contains_local([0, 1, 2].map(|k| p[k] - eps * nn[k]));
                            let outer = solid.contains_local([0, 1, 2].map(|k| p[k] + eps * nn[k]));
                            (inner != outer).then(|| if outer { nn.map(|v| -v) } else { nn })
                        };
                        if let Some(n) = on(p) {
                            samples.push(Sample { p, n, hu: du.map(|v| 0.5 * v), hv: dv.map(|v| 0.5 * v), area });
                            continue;
                        }
                        // A patch whose middle is off the body may still run onto it where another
                        // part ends it (a cutter's sheet across a blank's face): its quarters along
                        // the sheet, each tried on its own.
                        for f in [-0.75, -0.25, 0.25, 0.75] {
                            let q = [0, 1, 2].map(|k| p[k] + 0.5 * f * dv[k]);
                            if let Some(n) = on(q) {
                                samples.push(Sample { p: q, n, hu: du.map(|v| 0.5 * v), hv: dv.map(|v| 0.125 * v), area: 0.25 * area });
                            }
                        }
                    }
                }
            }
        }
        let sector = |s: &Sample| (((s.p[1].atan2(s.p[0]) + PI) / TAU * SECTORS as f64).floor() as usize).min(SECTORS - 1);
        samples.sort_by_key(sector);
        let mut sectors = Vec::with_capacity(SECTORS);
        let mut at = 0;
        for k in 0..SECTORS {
            let start = at;
            while at < samples.len() && sector(&samples[at]) == k {
                at += 1;
            }
            sectors.push(start..at);
        }
        Surface { samples, sectors, reach: 0.5 * h.hypot(hz) }
    }
    /// The samples near `other` as the two bodies are turned now, in world coordinates.
    fn facing(&self, solid: &Solid, other: &Solid, margin: f64) -> Vec<Sample> {
        let mut out = Vec::new();
        for r in &self.sectors {
            let s = &self.samples[r.clone()];
            // A sector none of whose scattered samples is anywhere near is skipped whole.
            let stride = (s.len() / 150).max(1);
            if !s.iter().step_by(stride).any(|q| other.near(solid.world(q.p), margin + 1.5)) {
                continue;
            }
            for q in s {
                let p = solid.world(q.p);
                if other.near(p, margin) {
                    out.push(Sample { p, n: solid.world_dir(q.n), hu: solid.world_dir(q.hu), hv: solid.world_dir(q.hv), area: q.area });
                }
            }
        }
        out
    }
}

/// How two bodies sit against each other.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stat {
    /// How deep the deepest surface point of one is inside the other (mm; 0: none is).
    pub penetration: f64,
    /// How near the nearest surface point of one is to the other's surface (mm; 0 when they
    /// penetrate, [`Stat::FAR`] when nothing is within reach).
    pub gap: f64,
    /// About how much volume the two share (mm³).
    pub volume: f64,
    /// Where the deepest point is (or, with none inside, the nearest): world coordinates.
    pub at: [f64; 3],
}

impl Stat {
    /// The gap reported when the surfaces are further apart than the check looks.
    pub const FAR: f64 = 1.0;
}

/// Each query sample against the target body: (deepest inside, nearest outside, shared volume).
fn one_way(queries: &[Sample], target: &Solid, ts: &[Sample], cell: f64) -> (f64, f64, f64, P3) {
    if queries.is_empty() {
        return (0.0, Stat::FAR, 0.0, [0.0; 3]);
    }
    let (mut at_deep, mut at_near) = ([0.0; 3], [0.0; 3]);
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for s in ts {
        for k in 0..3 {
            lo[k] = lo[k].min(s.p[k]);
            hi[k] = hi[k].max(s.p[k]);
        }
    }
    if ts.is_empty() {
        lo = [0.0; 3];
        hi = [0.0; 3];
    }
    let dims = if ts.is_empty() { [1usize; 3] } else { [0, 1, 2].map(|k| ((hi[k] - lo[k]) / cell).floor() as usize + 1) };
    let index = |p: P3| -> [i64; 3] { [0, 1, 2].map(|k| ((p[k] - lo[k]) / cell).floor() as i64) };
    let flat = |c: [i64; 3]| -> Option<usize> {
        if (0..3).all(|k| c[k] >= 0 && (c[k] as usize) < dims[k]) {
            Some((c[0] as usize * dims[1] + c[1] as usize) * dims[2] + c[2] as usize)
        } else {
            None
        }
    };
    // Counting sort of the target's samples into the grid's cells.
    let cells = dims[0] * dims[1] * dims[2];
    let mut start = vec![0u32; cells + 1];
    // Each sample with its cell (the grid spans them all; a sample that isn't a number has none
    // and is left out).
    let keys: Vec<(usize, usize)> = ts.iter().enumerate().filter_map(|(i, s)| Some((i, flat(index(s.p))?))).collect();
    for (_, k) in &keys {
        start[k + 1] += 1;
    }
    for i in 0..cells {
        start[i + 1] += start[i];
    }
    let mut order = vec![0u32; keys.len()];
    let mut fill = start.clone();
    for &(i, k) in &keys {
        order[fill[k] as usize] = i as u32;
        fill[k] += 1;
    }
    let (mut deep, mut near, mut volume) = (0.0f64, Stat::FAR, 0.0f64);
    for q in queries {
        let c = index(q.p);
        // The nearest patch (a sample with the parallelogram it stands for): how far, which
        // side of it, and whether the nearest point is within the patch or on its border.
        let mut best: Option<(f64, f64, bool)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(f) = flat([c[0] + dx, c[1] + dy, c[2] + dz]) else { continue };
                    for &i in &order[start[f] as usize..start[f + 1] as usize] {
                        let t = &ts[i as usize];
                        let d = sub(q.p, t.p);
                        if dot(d, d) > cell * cell {
                            continue;
                        }
                        let (uu, uv, vv) = (dot(t.hu, t.hu), dot(t.hu, t.hv), dot(t.hv, t.hv));
                        let (du, dv) = (dot(d, t.hu), dot(d, t.hv));
                        let det = (uu * vv - uv * uv).max(1e-300);
                        let (a, b) = ((du * vv - dv * uv) / det, (dv * uu - du * uv) / det);
                        let (ac, bc) = (a.clamp(-1.0, 1.0), b.clamp(-1.0, 1.0));
                        let r = [0, 1, 2].map(|k| d[k] - ac * t.hu[k] - bc * t.hv[k]);
                        let dist = dot(r, r).sqrt();
                        if best.is_none_or(|x| dist < x.0) {
                            best = Some((dist, dot(d, t.n), a == ac && b == bc));
                        }
                    }
                }
            }
        }
        let signed = match best {
            Some((dist, along, true)) => {
                // In front of the patch, or behind a patch near the body's edge but not in the
                // body (just past a convex edge, where a face's plane runs on beyond the body):
                // outside, by the body's own word. The body is asked only for depths the checks
                // could count, and only when the point is behind the patch.
                if along >= 0.0 || (dist > 1e-3 && !target.contains(q.p)) { dist } else { -dist }
            }
            // Nearest a patch's border (past an edge of the body, or of the sampled zone): the
            // body itself says which side, asked only when it could matter.
            Some((dist, along, false)) => {
                if dist >= near && dist < 0.5 * cell {
                    continue;
                } else if target.contains(q.p) {
                    // Inside, past the border of the nearest patch: as deep as it is behind
                    // that patch's plane. (Where a trimmed body's sampled surface stops a hair
                    // short of its edge, a point touching the edge is that hair from the patch
                    // along the surface, not into the body.)
                    -along.abs().min(dist)
                } else {
                    dist
                }
            }
            None => {
                if target.contains(q.p) {
                    -cell
                } else {
                    continue;
                }
            }
        };
        if signed < 0.0 {
            if -signed > deep {
                deep = -signed;
                at_deep = q.p;
            }
            volume += -signed * q.area;
        } else if signed < near {
            near = signed;
            at_near = q.p;
        }
    }
    (deep, near, volume, if deep > 0.0 { at_deep } else { at_near })
}

/// How the two bodies sit against each other as they are turned now.
pub fn mesh_stat(a: &Solid, sa: &Surface, b: &Solid, sb: &Surface) -> Stat {
    let cell = 0.3f64.max(2.0 * sa.reach).max(2.0 * sb.reach);
    let (wa, wb) = (sa.facing(a, b, cell), sb.facing(b, a, cell));
    let (d1, g1, v1, p1) = one_way(&wa, b, &wb, cell);
    let (d2, g2, v2, p2) = one_way(&wb, a, &wa, cell);
    let penetration = d1.max(d2);
    let at = if penetration > 0.0 {
        if d1 >= d2 { p1 } else { p2 }
    } else if g1 <= g2 {
        p1
    } else {
        p2
    };
    Stat { penetration, gap: if penetration > 0.0 { 0.0 } else { g1.min(g2) }, volume: 0.5 * (v1 + v2), at }
}

/// What a pair does over a stretch of its motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    /// The deepest penetration at any step, the driver angle it was at and where (world).
    pub penetration: f64,
    pub at: f64,
    pub place: [f64; 3],
    /// Where the gap was smallest (world), and at which driver angle.
    pub gap_place: [f64; 3],
    pub gap_at: f64,
    /// The smallest and the largest gap over the steps: both about the backlash when the teeth
    /// stay in mesh.
    pub gap_min: f64,
    pub gap_max: f64,
    /// The most volume shared at any step.
    pub volume: f64,
}

/// Two bodies with their surfaces sampled where they can meet.
pub struct Pair {
    pub a: Solid,
    pub b: Solid,
    sa: Surface,
    sb: Surface,
}

impl Pair {
    /// `h`: the spacing of the surface samples round the outlines (about 0.03 modules is good);
    /// along the sweeps they are `hz` apart.
    pub fn new(a: &Body, b: &Body, h: f64, hz: f64) -> Pair {
        let (a, b) = crate::par::join(|| Solid::new(a, h), || Solid::new(b, h));
        let (sa, sb) = crate::par::join(|| Surface::of(&a, &b, hz, None), || Surface::of(&b, &a, hz, None));
        Pair { a, b, sa, sb }
    }
    /// Like [`Pair::new`] for the motion [`Pair::run`] makes from `t0` to `t1` at `ratio` (no
    /// other turns): only the arcs of the bodies that come round to face each other are sampled.
    pub fn for_run(a: &Body, b: &Body, h: f64, hz: f64, ratio: f64, t0: f64, t1: f64) -> Pair {
        let (a, b) = crate::par::join(|| Solid::new(a, h), || Solid::new(b, h));
        let (b0, b1) = (ratio * t0, ratio * t1);
        let (sa, sb) =
            crate::par::join(|| Surface::of(&a, &b, hz, Some((t0.min(t1), t0.max(t1)))), || Surface::of(&b, &a, hz, Some((b0.min(b1), b0.max(b1)))));
        Pair { a, b, sa, sb }
    }
    /// The pair with the first body turned `ta` and the second `tb` about their own axes.
    pub fn at(&mut self, ta: f64, tb: f64) -> Stat {
        self.a.spin = ta;
        self.b.spin = tb;
        mesh_stat(&self.a, &self.sa, &self.b, &self.sb)
    }
    /// The pair stepped through its motion: the first body from `t0` to `t1` in `steps` steps,
    /// the second turned `ratio` times as far (plus `offset`).
    pub fn run(&mut self, ratio: f64, offset: f64, t0: f64, t1: f64, steps: usize) -> Run {
        let mut r = Run { penetration: 0.0, at: t0, place: [0.0; 3], gap_place: [0.0; 3], gap_at: t0, gap_min: f64::MAX, gap_max: 0.0, volume: 0.0 };
        // The steps at once (each on its own turned copies of the solids), then taken in order.
        let at = |i: usize| t0 + (t1 - t0) * i as f64 / steps as f64;
        let (pa, pb, sa, sb) = (&self.a, &self.b, &self.sa, &self.sb);
        let stats = crate::par::map(steps, |i| {
            let t = at(i);
            let (mut a, mut b) = (pa.clone(), pb.clone());
            a.spin = t;
            b.spin = ratio * t + offset;
            mesh_stat(&a, sa, &b, sb)
        });
        if steps > 0 {
            let t = at(steps - 1);
            self.a.spin = t;
            self.b.spin = ratio * t + offset;
        }
        for (i, s) in stats.into_iter().enumerate() {
            let t = at(i);
            if s.penetration > r.penetration {
                r.penetration = s.penetration;
                r.at = t;
                r.place = s.at;
            }
            if s.gap < r.gap_min && s.penetration == 0.0 {
                r.gap_place = s.at;
                r.gap_at = t;
            }
            r.gap_min = r.gap_min.min(s.gap);
            r.gap_max = r.gap_max.max(s.gap);
            r.volume = r.volume.max(s.volume);
        }
        r
    }
    pub fn samples(&self) -> (usize, usize) {
        (self.sa.samples.len(), self.sb.samples.len())
    }
}

/// The volume (mm³) two bodies share, counted on a jittered grid of cells about `cell` across
/// over the zone both can reach: slower than [`mesh_stat`]'s estimate and blind to anything
/// thinner than a cell, but independent of it.
pub fn overlap(a: &Solid, b: &Solid, cell: f64) -> f64 {
    let ((la, ha), (lb, hb)) = (a.bounds(), b.bounds());
    let lo = [0, 1, 2].map(|i| la[i].max(lb[i]));
    let hi = [0, 1, 2].map(|i| ha[i].min(hb[i]));
    let size = [0, 1, 2].map(|i| (hi[i] - lo[i]).max(0.0));
    let vol = size[0] * size[1] * size[2];
    if vol <= 0.0 {
        return 0.0;
    }
    let cell = cell.max((vol / 6.0e6).cbrt());
    let n = [0, 1, 2].map(|i| (size[i] / cell).ceil().max(1.0) as usize);
    let d = [0, 1, 2].map(|i| size[i] / n[i] as f64);
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut count = 0usize;
    for i in 0..n[0] {
        for j in 0..n[1] {
            for k in 0..n[2] {
                let p = [lo[0] + (i as f64 + rnd()) * d[0], lo[1] + (j as f64 + rnd()) * d[1], lo[2] + (k as f64 + rnd()) * d[2]];
                if a.contains(p) && b.contains(p) {
                    count += 1;
                }
            }
        }
    }
    count as f64 * d[0] * d[1] * d[2]
}

// ---------------------------------------------------------------------------------------------
// The plan's 3D meshes, checked for the window.

/// How a pair whose axes are not parallel turns: member 2's turn per radian of member 1's
/// (bodies in plan order: gear 1 and gear 2, or the wheel and then the worm), half a pitch of
/// member 2, and one tooth pitch of member 1 (the driver angle after which the mesh repeats).
pub fn motion(s: &Spec) -> (f64, f64, f64) {
    match s.kind {
        // A right-hand worm turned forwards carries the wheel back.
        Kind::Worm => {
            let hand = if s.helix < 0.0 { -1.0 } else { 1.0 };
            let starts = f64::from(s.starts.max(1));
            (-hand * f64::from(s.teeth) / starts, PI / starts, TAU / f64::from(s.teeth.max(1)))
        }
        // A face gear's set is the face gear (teeth2) first, then its pinion (teeth): the pinion
        // turns teeth2/teeth times as fast. Turned the other way round, the check read the pair
        // as interfering everywhere but the built pose.
        Kind::FaceGear => (-f64::from(s.teeth2) / f64::from(s.teeth.max(1)), PI / f64::from(s.teeth.max(1)), TAU / f64::from(s.teeth2.max(1))),
        _ => (-f64::from(s.teeth) / f64::from(s.teeth2.max(1)), PI / f64::from(s.teeth2.max(1)), TAU / f64::from(s.teeth.max(1))),
    }
}

/// The least flank gap the backlash asks for, square to the flanks (mm): the smallest and the
/// largest anywhere on the face. Every member's teeth are thinner by the backlash on its pitch
/// circle (a bevel's at its large end; a worm's thread and its wheel's teeth by half of it each,
/// along the worm's axis), and the pair is turned to the middle of the play, so each side has
/// (j/2)·cos αn·cos β of each member's thinning, β the member's helix (spiral, lead) angle.
pub fn expected_gap(s: &Spec) -> (f64, f64) {
    // Member 2 (gear 2, the worm) may have a backlash of its own.
    let (j, j2, an) = (s.backlash, s.backlash_of2(), s.pressure_angle.to_radians());
    match s.kind {
        Kind::CrossedHelical => {
            let (b1, b2) = (s.helix.to_radians(), (s.shaft_angle - s.helix).to_radians());
            let g = 0.5 * an.cos() * (j * b1.cos() + j2 * b2.cos());
            (g, g)
        }
        Kind::Bevel | Kind::SpiralBevel => {
            let sigma = s.shaft_angle.to_radians();
            let (z1, z2) = (f64::from(s.teeth), f64::from(s.teeth2));
            let d1 = sigma.sin().atan2(z2 / z1 + sigma.cos());
            let re = s.module * z1 / 2.0 / d1.sin();
            let beta = if s.kind == Kind::SpiralBevel { s.spiral_angle.to_radians() } else { 0.0 };
            // The same turn of play is less gap nearer the apex: over the stretch of the cone
            // both members' teeth are on.
            let g = j * an.cos() * beta.cos();
            let (lo, hi) = crate::plan::faces_overlap(s, re);
            (g * lo / re, g * hi / re)
        }
        Kind::Worm => {
            let d1 = if s.worm_diameter > 0.0 { s.worm_diameter } else { 10.0 * s.module };
            let gamma = (s.module * f64::from(s.starts.max(1)) / d1).atan();
            let g = 0.25 * (j + j2) * an.cos() * gamma.cos();
            (g, g)
        }
        Kind::BevelPlanetary => crate::plan::bevel_planet::expected_gap(s),
        _ => (0.0, j * an.cos()),
    }
}

/// Two bodies of a set whose axes are not parallel that mesh, for [`check`], and which preview
/// members they are.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshPair3 {
    /// Indices into `Plan::preview` (gear 1 and gear 2; the wheel and the worm).
    pub a: usize,
    pub b: usize,
    /// The whole set's two members when only one is built (the plan's bodies otherwise).
    pub bodies: Option<[Body; 2]>,
    /// Member 2's turn per radian of member 1's, and member 1's tooth pitch (radians).
    pub ratio: f64,
    pub period: f64,
    /// The flank gap the backlash asks for (least and most over the face; mm).
    pub gap: (f64, f64),
    /// The length tolerances scale with (the module; mm).
    pub scale: f64,
    /// The two members' names when they aren't the preview's `a` and `b` (a bevel planetary
    /// set's preview shows one planet; its meshes name every planet).
    pub labels: Option<[String; 2]>,
}

/// The 3D meshes of a planned set whose axes are not parallel: `whole`, the whole set's two
/// bodies when the plan has only one of them.
pub fn topology(s: &Spec, planned: &[Body], whole: Option<Vec<Body>>) -> Vec<MeshPair3> {
    if !matches!(s.kind, Kind::Bevel | Kind::SpiralBevel | Kind::Hypoid | Kind::CrossedHelical | Kind::Worm) {
        return Vec::new();
    }
    let bodies = match whole {
        Some(w) if w.len() == 2 => Some([w[0].clone(), w[1].clone()]),
        Some(_) => return Vec::new(),
        None if planned.len() == 2 => None,
        None => return Vec::new(),
    };
    let (ratio, _, period) = motion(s);
    vec![MeshPair3 { a: 0, b: 1, bodies, ratio, period, gap: expected_gap(s), scale: s.module, labels: None }]
}

/// Driver angles [`check`] probes over one tooth pitch.
pub const CHECK_STEPS: usize = 6;

/// Checks a 3D mesh over one tooth pitch of member 1 with coarse surface samples (a tenth of a
/// module apart round the outlines, a third along the sweeps; about 30 to 150 ms for the
/// default sets, so debounce it as the 2D check): overlap deeper than
/// [`crate::mesh2d::CHECK_DEPTH`] is interference, flanks further apart than the backlash asks
/// plus [`crate::mesh2d::CHECK_GAP`] have lost contact.
pub fn check(plan: &Plan, m: &MeshPair3) -> MeshResult {
    use crate::mesh2d::{CHECK_DEPTH, CHECK_GAP};
    let (ba, bb) = match &m.bodies {
        Some(b) => (&b[0], &b[1]),
        None => (&plan.bodies[0], &plan.bodies[1]),
    };
    let mut pair = Pair::for_run(ba, bb, m.scale / 10.0, m.scale / 3.0, m.ratio, -0.5 * m.period, 0.5 * m.period);
    let r = pair.run(m.ratio, 0.0, -0.5 * m.period, 0.5 * m.period, CHECK_STEPS);
    let name = |i: usize| plan.preview.get(i).map_or_else(|| format!("member {}", i + 1), |g| g.name.clone());
    let (na, nb) = match &m.labels {
        Some([a, b]) => (a.clone(), b.clone()),
        None => (name(m.a), name(m.b)),
    };
    let (depth, gap_min, gap_max) = (r.penetration, if r.gap_min.is_finite() { r.gap_min } else { 0.0 }, r.gap_max);
    let (verdict, message) = if depth > CHECK_DEPTH * m.scale {
        (Verdict::Interferes, format!("{na} and {nb} interfere: they overlap by {depth:.3} mm"))
    } else if gap_max > m.gap.1 + CHECK_GAP * m.scale {
        (Verdict::NoContact, format!("{na} and {nb} lose contact: up to {gap_max:.3} mm apart"))
    } else if m.gap.1 > 0.0 {
        (Verdict::Ok, format!("{na} and {nb} mesh: no overlap, flanks {gap_min:.3} to {gap_max:.3} mm apart"))
    } else {
        (Verdict::Ok, format!("{na} and {nb} mesh: no overlap, flanks in contact"))
    };
    MeshResult { a: m.a, b: m.b, penetration_mm: depth, gap_min_mm: gap_min, gap_max_mm: gap_max, verdict, message }
}
