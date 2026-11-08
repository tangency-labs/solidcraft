//! Exact surfaces of B-rep faces: planes, circular cylinders and cones, spheres,
//! tori and rational B-spline surfaces, with the surface's own normal and the
//! closest point of an untrimmed surface.
//!
//! Analytic surfaces use the STEP (ISO 10303-42) parameterizations:
//!   cylinder  `o + r e_r(u) + v z`
//!   cone      `o + (r + v tan a) e_r(u) + v z`       (a = semi-angle)
//!   sphere    `o + r cos v e_r(u) + r sin v z`
//!   torus     `o + (R + r cos v) e_r(u) + r sin v z`
//! with `e_r(u) = cos u x + sin u y` in the surface frame. The surface normal is
//! the STEP one (outward from the axis, center or tube center; Su x Sv for
//! B-spline surfaces).
//!
//! Each analytic surface has exactly one local minimum of the distance to a
//! point (the foot), so the closest point of a trimmed face is its foot when
//! the foot lies in the face and on the face's boundary otherwise. B-spline
//! surfaces can have several; their closest point is a certified branch and
//! bound over Bezier patches.

use std::sync::Arc;

use crate::geom3::{clamp, insert_knot, Frame, V3};

/// An exact surface: analytic in its frame, or a rational B-spline surface.
#[derive(Clone, Debug)]
pub enum Surf {
    /// The plane through the frame's origin, normal to its z axis.
    Plane {
        /// The surface frame.
        f: Frame,
    },
    /// A circular cylinder about the frame's z axis.
    Cylinder {
        /// The surface frame.
        f: Frame,
        /// The radius.
        r: f64,
    },
    /// A circular cone about the frame's z axis.
    Cone {
        /// The surface frame.
        f: Frame,
        /// The radius at height 0.
        r: f64,
        /// The semi-angle (radians).
        a: f64,
    },
    /// A sphere about the frame's origin.
    Sphere {
        /// The surface frame.
        f: Frame,
        /// The radius.
        r: f64,
    },
    /// A torus about the frame's z axis.
    Torus {
        /// The surface frame.
        f: Frame,
        /// The radius of the tube's center circle.
        major: f64,
        /// The tube's radius.
        minor: f64,
    },
    /// A rational B-spline surface.
    Nurbs(Arc<NurbsSurf>),
}

/// Axial coordinates of p in a frame: angle about z, distance from the axis,
/// height along z, and the radial and tangential unit vectors at that angle.
#[derive(Clone, Copy, Debug)]
struct Axial {
    rho: f64,
    h: f64,
    er: V3,
}

fn axial(f: &Frame, p: V3) -> Axial {
    let l = f.local(p);
    let rho = (l.x * l.x + l.y * l.y).sqrt();
    let theta = if rho > 0.0 { l.y.atan2(l.x) } else { 0.0 };
    let (s, c) = theta.sin_cos();
    Axial {
        rho,
        h: l.z,
        er: f.dir(V3::new(c, s, 0.0)),
    }
}

impl Surf {
    /// The kind of surface, for messages.
    pub fn name(&self) -> &'static str {
        match self {
            Surf::Plane { .. } => "plane",
            Surf::Cylinder { .. } => "cylinder",
            Surf::Cone { .. } => "cone",
            Surf::Sphere { .. } => "sphere",
            Surf::Torus { .. } => "torus",
            Surf::Nurbs(_) => "B-spline surface",
        }
    }

    /// The frame of an analytic surface.
    pub fn frame(&self) -> Option<&Frame> {
        match self {
            Surf::Plane { f }
            | Surf::Cylinder { f, .. }
            | Surf::Cone { f, .. }
            | Surf::Sphere { f, .. }
            | Surf::Torus { f, .. } => Some(f),
            Surf::Nurbs(_) => None,
        }
    }

    /// Apex of a cone (height -r / tan a on its axis).
    pub fn apex(&self) -> Option<V3> {
        match self {
            Surf::Cone { f, r, a } => Some(f.world(V3::new(0.0, 0.0, -r / a.tan()))),
            _ => None,
        }
    }

    /// The foot of p on an analytic surface: its unique local minimum of distance.
    /// On a degenerate position (on the axis, at the center) any closest point.
    /// For a cone, the nappe r + v tan a >= 0 (the apex when p is behind it).
    pub fn foot(&self, p: V3) -> V3 {
        match self {
            Surf::Plane { f } => p - f.z * (p - f.o).dot(f.z),
            Surf::Cylinder { f, r } => {
                let ax = axial(f, p);
                f.o + ax.er * *r + f.z * ax.h
            }
            Surf::Cone { f, r, a } => {
                let ax = axial(f, p);
                let (sa, ca) = a.sin_cos();
                let ha = -r / a.tan();
                let s = ax.rho * sa + (ax.h - ha) * ca;
                if s <= 0.0 {
                    f.o + f.z * ha
                } else {
                    f.o + ax.er * (s * sa) + f.z * (ha + s * ca)
                }
            }
            Surf::Sphere { f, r } => {
                let d = p - f.o;
                let l = d.len();
                if l > 0.0 {
                    f.o + d * (r / l)
                } else {
                    f.o + f.x * *r
                }
            }
            Surf::Torus { f, major, minor } => {
                let ax = axial(f, p);
                let c = f.o + ax.er * *major;
                let d = p - c;
                let l = d.len();
                if l > 0.0 {
                    c + d * (minor / l)
                } else {
                    c + ax.er * *minor
                }
            }
            Surf::Nurbs(n) => n.closest(p, f64::INFINITY).map_or(p, |h| h.q),
        }
    }

    /// The surface's own unit normal at the surface point nearest x.
    pub fn normal(&self, x: V3) -> V3 {
        match self {
            Surf::Plane { f } => f.z,
            Surf::Cylinder { f, .. } => axial(f, x).er,
            Surf::Cone { f, a, .. } => {
                let ax = axial(f, x);
                let (sa, ca) = a.sin_cos();
                ax.er * ca - f.z * sa
            }
            Surf::Sphere { f, .. } => (x - f.o).unit(),
            Surf::Torus { f, major, .. } => {
                let ax = axial(f, x);
                (x - (f.o + ax.er * *major)).unit()
            }
            Surf::Nurbs(n) => n
                .closest(x, f64::INFINITY)
                .map_or(V3::new(0.0, 0.0, 1.0), |h| h.n),
        }
    }

    /// The foot of p and the surface's own unit normal there, from one search.
    pub fn foot_normal(&self, p: V3) -> (V3, V3) {
        if let Surf::Nurbs(n) = self {
            n.closest(p, f64::INFINITY)
                .map_or((p, V3::new(0.0, 0.0, 1.0)), |h| (h.q, h.n))
        } else {
            let q = self.foot(p);
            (q, self.normal(q))
        }
    }

    /// Distance from x to the untrimmed surface (for tolerance checks).
    pub fn dist(&self, x: V3) -> f64 {
        (x - self.foot(x)).len()
    }
}

// ---------------------------------------------------------------- B-spline surfaces

/// Coefficient i of a basis level, zero outside it (the triangle's edges).
fn coef(level: &[f64], i: isize) -> f64 {
    usize::try_from(i)
        .ok()
        .and_then(|i| level.get(i))
        .copied()
        .unwrap_or(0.0)
}

/// Bernstein basis of degree n at t with first and second derivatives.
fn bernstein(n: usize, t: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let mut levels: Vec<Vec<f64>> = vec![vec![1.0]];
    for k in 1..=n {
        let prev = levels.last().map_or(&[][..], Vec::as_slice);
        let b = (0..=k)
            .map(|i| {
                let left = if i > 0 {
                    coef(prev, i as isize - 1)
                } else {
                    0.0
                };
                let right = if i < k { coef(prev, i as isize) } else { 0.0 };
                (1.0 - t) * right + t * left
            })
            .collect();
        levels.push(b);
    }
    let level = |m: Option<usize>| m.and_then(|m| levels.get(m)).map_or(&[][..], Vec::as_slice);
    let (l1, l2) = (level(n.checked_sub(1)), level(n.checked_sub(2)));
    let mut d1 = vec![0.0; n + 1];
    let mut d2 = vec![0.0; n + 1];
    for (i, (d1i, d2i)) in d1.iter_mut().zip(&mut d2).enumerate() {
        let ii = i as isize;
        if n >= 1 {
            *d1i = n as f64 * (coef(l1, ii - 1) - coef(l1, ii));
        }
        if n >= 2 {
            *d2i =
                (n * (n - 1)) as f64 * (coef(l2, ii - 2) - 2.0 * coef(l2, ii - 1) + coef(l2, ii));
        }
    }
    (levels.pop().unwrap_or_default(), d1, d2)
}

/// Degrees below this evaluate on the stack.
const BMAX: usize = 16;

/// Bernstein basis of degree n < BMAX at t with first and second derivatives,
/// without allocating.
fn bernstein_small(n: usize, t: f64) -> ([f64; BMAX], [f64; BMAX], [f64; BMAX]) {
    // Levels n - 2 and n - 1 of the triangle are kept for the derivatives.
    let (mut cur, mut l1, mut l2) = ([0.0f64; BMAX], [0.0f64; BMAX], [0.0f64; BMAX]);
    cur[0] = 1.0;
    if n == 1 {
        l1 = cur;
    }
    if n == 2 {
        l2 = cur;
    }
    for k in 1..=n {
        for i in (0..=k).rev() {
            let left = if i > 0 {
                coef(&cur, i as isize - 1)
            } else {
                0.0
            };
            let right = if i < k { coef(&cur, i as isize) } else { 0.0 };
            if let Some(c) = cur.get_mut(i) {
                *c = (1.0 - t) * right + t * left;
            }
        }
        if n >= 1 && k == n - 1 {
            l1 = cur;
        }
        if n >= 2 && k == n - 2 {
            l2 = cur;
        }
    }
    // Level n - 1 has n coefficients and level n - 2 has n - 1.
    let (l1, l2) = (
        l1.get(..n).unwrap_or_default(),
        l2.get(..n.saturating_sub(1)).unwrap_or_default(),
    );
    let (mut d1, mut d2) = ([0.0f64; BMAX], [0.0f64; BMAX]);
    for (i, (d1i, d2i)) in d1.iter_mut().zip(&mut d2).enumerate().take(n + 1) {
        let ii = i as isize;
        if n >= 1 {
            *d1i = n as f64 * (coef(l1, ii - 1) - coef(l1, ii));
        }
        if n >= 2 {
            *d2i =
                (n * (n - 1)) as f64 * (coef(l2, ii - 2) - 2.0 * coef(l2, ii - 1) + coef(l2, ii));
        }
    }
    (cur, d1, d2)
}

/// Point, first and second derivatives of a surface at local (s, t).
#[derive(Clone, Copy, Debug)]
pub struct SEval {
    /// The point.
    pub p: V3,
    /// The first derivative in s.
    pub ps: V3,
    /// The first derivative in t.
    pub pt: V3,
    /// The second derivative in s.
    pub pss: V3,
    /// The mixed second derivative.
    pub pst: V3,
    /// The second derivative in t.
    pub ptt: V3,
}

/// A rational Bezier patch in homogeneous form: `pw[i][j]`, i along u.
#[derive(Clone, Debug)]
struct Patch {
    pw: Vec<Vec<[f64; 4]>>,
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
}

fn v3h(h: [f64; 4]) -> V3 {
    V3::new(h[0], h[1], h[2])
}

fn casteljau_split(pts: &[[f64; 4]], s: f64) -> (Vec<[f64; 4]>, Vec<[f64; 4]>) {
    let n = pts.len().saturating_sub(1);
    let mut b = pts.to_vec();
    let (mut left, mut right): (Vec<_>, Vec<_>) = (
        b.first().into_iter().copied().collect(),
        b.last().into_iter().copied().collect(),
    );
    for level in 1..=n {
        for i in 0..=(n - level) {
            if let Some([cur, next]) = b.get_mut(i..i + 2) {
                for (c, x) in cur.iter_mut().zip(*next) {
                    *c = (1.0 - s) * *c + s * x;
                }
            }
        }
        left.extend(b.first());
        right.extend(b.get(n - level));
    }
    right.reverse();
    (left, right)
}

impl Patch {
    fn ctrl(&self) -> impl Iterator<Item = V3> + '_ {
        self.pw
            .iter()
            .flatten()
            .map(|h| V3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]))
    }
    fn bbox(&self) -> (V3, V3) {
        let mut lo = V3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = V3::new(f64::MIN, f64::MIN, f64::MIN);
        for q in self.ctrl() {
            lo = V3::new(lo.x.min(q.x), lo.y.min(q.y), lo.z.min(q.z));
            hi = V3::new(hi.x.max(q.x), hi.y.max(q.y), hi.z.max(q.z));
        }
        (lo, hi)
    }
    fn eval(&self, s: f64, t: f64) -> SEval {
        let du = self.pw.len().saturating_sub(1);
        let dv = self.pw.first().map_or(0, Vec::len).saturating_sub(1);
        if du < BMAX && dv < BMAX {
            let (bu, bu1, bu2) = bernstein_small(du, s);
            let (bv, bv1, bv2) = bernstein_small(dv, t);
            return self.combine([&bu, &bu1, &bu2], [&bv, &bv1, &bv2]);
        }
        let (bu, bu1, bu2) = bernstein(du, s);
        let (bv, bv1, bv2) = bernstein(dv, t);
        self.combine([&bu, &bu1, &bu2], [&bv, &bv1, &bv2])
    }

    /// The point and derivatives from the basis in u and in v (each with its
    /// first and second derivatives).
    fn combine(&self, [bu, bu1, bu2]: [&[f64]; 3], [bv, bv1, bv2]: [&[f64]; 3]) -> SEval {
        let mut a = [[0.0f64; 4]; 6]; // A, As, At, Ass, Ast, Att
        for (row, ((&b0, &b1), &b2)) in self.pw.iter().zip(bu.iter().zip(bu1).zip(bu2)) {
            for (h, ((&c0, &c1), &c2)) in row.iter().zip(bv.iter().zip(bv1).zip(bv2)) {
                let w = [b0 * c0, b1 * c0, b0 * c1, b2 * c0, b1 * c1, b0 * c2];
                for (ak, wk) in a.iter_mut().zip(w) {
                    for (akc, hc) in ak.iter_mut().zip(h) {
                        *akc += wk * hc;
                    }
                }
            }
        }
        let w = a[0][3];
        let p = v3h(a[0]) * (1.0 / w);
        let ps = (v3h(a[1]) - p * a[1][3]) * (1.0 / w);
        let pt = (v3h(a[2]) - p * a[2][3]) * (1.0 / w);
        let pss = (v3h(a[3]) - ps * (2.0 * a[1][3]) - p * a[3][3]) * (1.0 / w);
        let pst = (v3h(a[4]) - ps * a[2][3] - pt * a[1][3] - p * a[4][3]) * (1.0 / w);
        let ptt = (v3h(a[5]) - pt * (2.0 * a[2][3]) - p * a[5][3]) * (1.0 / w);
        SEval {
            p,
            ps,
            pt,
            pss,
            pst,
            ptt,
        }
    }

    fn split_u(&self, s: f64) -> (Patch, Patch) {
        let dv = self.pw.first().map_or(0, Vec::len).saturating_sub(1);
        let mut l = vec![vec![[0.0; 4]; dv + 1]; self.pw.len()];
        let mut r = l.clone();
        for j in 0..=dv {
            let col: Vec<[f64; 4]> = self
                .pw
                .iter()
                .filter_map(|row| row.get(j).copied())
                .collect();
            let (a, b) = casteljau_split(&col, s);
            for ((lr, rr), (ai, bi)) in l.iter_mut().zip(&mut r).zip(a.into_iter().zip(b)) {
                if let (Some(lj), Some(rj)) = (lr.get_mut(j), rr.get_mut(j)) {
                    *lj = ai;
                    *rj = bi;
                }
            }
        }
        let um = self.u0 + s * (self.u1 - self.u0);
        (
            Patch {
                pw: l,
                u1: um,
                ..self.clone()
            },
            Patch {
                pw: r,
                u0: um,
                ..self.clone()
            },
        )
    }
    fn split_v(&self, t: f64) -> (Patch, Patch) {
        let (mut l, mut r) = (Vec::new(), Vec::new());
        for row in &self.pw {
            let (a, b) = casteljau_split(row, t);
            l.push(a);
            r.push(b);
        }
        let vm = self.v0 + t * (self.v1 - self.v0);
        (
            Patch {
                pw: l,
                v1: vm,
                ..self.clone()
            },
            Patch {
                pw: r,
                v0: vm,
                ..self.clone()
            },
        )
    }

    /// Newton on |S - p|^2 over [0, 1]^2 from (s, t) with at most `iters`
    /// steps, Gauss-Newton where the Hessian is not positive definite: enough
    /// for a branch and bound, which only needs a good point (its bounds
    /// certify).
    fn newton_capped(&self, p: V3, mut s: f64, mut t: f64, iters: usize) -> (f64, f64) {
        let (sr, tr) = ((0.0, 1.0), (0.0, 1.0));
        // A parameter sitting on its bound with its step pointing out is held
        // there, and Newton continues in the other one alone (the minimum along
        // that side of the patch); clamping both each step would bounce off the
        // bound until the iteration cap.
        let (mut hold_s, mut hold_t) = (false, false);
        for _ in 0..iters {
            let e = self.eval(s, t);
            let r = e.p - p;
            let g = [e.ps.dot(r), e.pt.dot(r)];
            let (jss, jst, jtt) = (e.ps.dot(e.ps), e.ps.dot(e.pt), e.pt.dot(e.pt));
            let mut h = [jss + e.pss.dot(r), jst + e.pst.dot(r), jtt + e.ptt.dot(r)];
            let (ds, dt) = match (hold_s, hold_t) {
                (true, true) => break,
                (true, false) => (0.0, -g[1] / if h[2] > 0.0 { h[2] } else { jtt.max(1e-300) }),
                (false, true) => (-g[0] / if h[0] > 0.0 { h[0] } else { jss.max(1e-300) }, 0.0),
                (false, false) => {
                    if !(h[0] > 0.0 && h[0] * h[2] - h[1] * h[1] > 0.0) {
                        h = [jss, jst, jtt];
                    }
                    let det = h[0] * h[2] - h[1] * h[1];
                    if det.abs() < 1e-300 {
                        break;
                    }
                    (
                        -(h[2] * g[0] - h[1] * g[1]) / det,
                        -(h[0] * g[1] - h[1] * g[0]) / det,
                    )
                }
            };
            let at_bound =
                |x: f64, d: f64, r: (f64, f64)| (x <= r.0 && d < 0.0) || (x >= r.1 && d > 0.0);
            if !hold_s && at_bound(s, ds, sr) {
                hold_s = true;
                continue;
            }
            if !hold_t && at_bound(t, dt, tr) {
                hold_t = true;
                continue;
            }
            let (ns, nt) = ((s + ds).clamp(sr.0, sr.1), (t + dt).clamp(tr.0, tr.1));
            let moved = (ns - s).abs() + (nt - t).abs();
            s = ns;
            t = nt;
            if moved < 1e-13 {
                break;
            }
        }
        (s, t)
    }

    /// The four quarters of the patch (split at the middle in u and in v).
    fn quarters(&self) -> [Patch; 4] {
        let (a, b) = self.split_u(0.5);
        let (a0, a1) = a.split_v(0.5);
        let (b0, b1) = b.split_v(0.5);
        [a0, a1, b0, b1]
    }
}

/// Bezier points of the single knot span `knots[p] .. knots[p + 1]` of a local
/// B-spline piece (2p + 2 knots, p + 1 points): raise both span ends to
/// multiplicity p by knot insertion; the p + 1 points ending at the last copy of
/// the span's start are then its Bezier points. None when the counts do not
/// describe such a piece.
fn span_bezier<const N: usize>(p: usize, knots: &[f64], pts: &[[f64; N]]) -> Option<Vec<[f64; N]>> {
    let (a, b) = (*knots.get(p)?, *knots.get(p + 1)?);
    let (mut kn, mut pt) = (knots.to_vec(), pts.to_vec());
    while kn.iter().filter(|&&x| x == a).count() < p {
        insert_knot(p, &mut kn, &mut pt, a)?;
    }
    while kn.iter().filter(|&&x| x == b).count() < p {
        insert_knot(p, &mut kn, &mut pt, b)?;
    }
    let last_a = kn.iter().rposition(|&x| x == a)?;
    pt.get(last_a.checked_sub(p)?..=last_a).map(<[_]>::to_vec)
}

/// Node of the quadtree over a B-spline surface's grid of knot spans.
#[derive(Clone, Debug)]
struct QNode {
    lo: V3,
    hi: V3,
    /// The first span along u and along v (a leaf's span).
    i0: u32,
    j0: u32,
    kids: Vec<u32>,
}

/// The deepest quadtree over a span grid: each level halves both span ranges,
/// so a grid of up to 2^32 spans a side is down to single spans in 33 levels.
const MAX_TREE_DEPTH: u32 = 40;

/// A clamped rational B-spline surface, kept as its control net. Bezier patches
/// are extracted one knot span at a time when a search reaches them (a surface
/// like the brain coral's, 363 thousand control points at degree 5, would need
/// hundreds of megabytes as patches). Each span is bounded by the box of the
/// control points that act on it (convex hull property, positive weights), and a
/// quadtree over the span grid carries the union boxes.
#[derive(Clone, Debug)]
pub struct NurbsSurf {
    du: usize,
    dv: usize,
    nv: usize,
    /// Homogeneous control net, point (i, j) at i * nv + j.
    pw: Vec<[f64; 4]>,
    ku: Vec<f64>,
    kv: Vec<f64>,
    /// Knot index s of each nonempty span ku[s] .. ku[s + 1] (and alike in v).
    su: Vec<usize>,
    sv: Vec<usize>,
    nodes: Vec<QNode>,
}

/// A point of a B-spline surface nearest a query.
#[derive(Clone, Copy, Debug)]
pub struct SurfHit {
    /// The point.
    pub q: V3,
    /// Its distance from the query.
    pub dist: f64,
    /// Its u parameter.
    pub u: f64,
    /// Its v parameter.
    pub v: f64,
    /// Unit normal Su x Sv at the hit.
    pub n: V3,
}

/// What a branch and bound entry holds: a quadtree node, or an extracted patch.
enum Entry {
    Node(u32),
    Patch(Patch),
}

/// Min-heap entry for branch and bound over a B-spline surface.
struct PatchItem {
    lb: f64,
    entry: Entry,
    depth: u32,
}
impl PartialEq for PatchItem {
    fn eq(&self, o: &Self) -> bool {
        self.lb == o.lb
    }
}
impl Eq for PatchItem {}
impl PartialOrd for PatchItem {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for PatchItem {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.lb.total_cmp(&self.lb)
    }
}

/// Newton steps per patch inside a branch and bound search.
const SEARCH_NEWTON: usize = 12;

/// Distances below this (mm) are treated as settled by the branch and bound searches.
const DIST_EPS: f64 = 1e-9;

fn box_dist(p: V3, b: &(V3, V3)) -> f64 {
    V3::new(
        (b.0.x - p.x).max(p.x - b.1.x).max(0.0),
        (b.0.y - p.y).max(p.y - b.1.y).max(0.0),
        (b.0.z - p.z).max(p.z - b.1.z).max(0.0),
    )
    .len()
}

/// Lower bound on the distance from p to a patch along unit direction n: the
/// patch lies in the hull of its control points, and every hull point y has
/// (p - y).n at least the least (p - c).n over the control points. Taken along
/// the direction to the patch's Newton point, it tightens quadratically as the
/// patch flattens, where a box bound only tightens linearly. It also holds for
/// every sub-patch (their control hulls lie in the parent's).
fn slab_bound(pa: &Patch, p: V3, n: V3) -> f64 {
    pa.ctrl()
        .map(|c| (p - c).dot(n))
        .fold(f64::INFINITY, f64::min)
}

impl NurbsSurf {
    /// Rows `ctrl[i][j]` with i along u, weights alike, clamped knot vectors.
    pub fn new(
        du: usize,
        dv: usize,
        ctrl: &[Vec<V3>],
        weights: Option<&Vec<Vec<f64>>>,
        ku: &[f64],
        kv: &[f64],
    ) -> Result<NurbsSurf, String> {
        let nu = ctrl.len();
        let nv = ctrl.first().map_or(0, Vec::len);
        if nu + du + 1 != ku.len() || nv + dv + 1 != kv.len() || ctrl.iter().any(|r| r.len() != nv)
        {
            return Err(format!("B-spline surface: {nu} x {nv} control points do not match degrees {du}, {dv} and {} + {} knots", ku.len(), kv.len()));
        }
        if weights.is_some_and(|w| w.len() != nu || w.iter().any(|r| r.len() != nv)) {
            return Err("B-spline surface: weights do not match its control points".into());
        }
        if weights.is_some_and(|w| w.iter().flatten().any(|&x| !(x > 0.0))) {
            return Err("B-spline surface with a weight that is not positive".into());
        }
        let bad = || format!("B-spline surface: knots do not describe degrees {du}, {dv}");
        // Homogeneous net, clamped exactly in each direction (knot insertion leaves
        // the surface unchanged; every row takes the same insertions).
        let h = |c: &V3, wt: f64| [c.x * wt, c.y * wt, c.z * wt, wt];
        let mut net: Vec<Vec<[f64; 4]>> = match weights {
            Some(w) => ctrl
                .iter()
                .zip(w)
                .map(|(row, ws)| row.iter().zip(ws).map(|(c, &wt)| h(c, wt)).collect())
                .collect(),
            None => ctrl
                .iter()
                .map(|row| row.iter().map(|c| h(c, 1.0)).collect())
                .collect(),
        };
        let mut kv2 = kv.to_vec();
        for row in &mut net {
            let (k, r) = clamp(dv, kv, row).ok_or_else(bad)?;
            kv2 = k;
            *row = r;
        }
        let nv = kv2.len().checked_sub(dv + 1).ok_or_else(bad)?;
        let mut ku2 = ku.to_vec();
        let mut cols: Vec<Vec<[f64; 4]>> = (0..nv)
            .map(|j| net.iter().map(|r| r.get(j).copied()).collect::<Option<_>>())
            .collect::<Option<_>>()
            .ok_or_else(bad)?;
        for col in &mut cols {
            let (k, c) = clamp(du, ku, col).ok_or_else(bad)?;
            ku2 = k;
            *col = c;
        }
        let nu = ku2.len().checked_sub(du + 1).ok_or_else(bad)?;
        let (ku, kv) = (&ku2[..], &kv2[..]);
        let mut pw = Vec::with_capacity(nu * nv);
        for i in 0..nu {
            for col in &cols {
                pw.push(*col.get(i).ok_or_else(bad)?);
            }
        }
        let spans = |k: &[f64], d: usize| -> Vec<usize> {
            (d..k.len().saturating_sub(d + 1))
                .filter(|&s| matches!((k.get(s), k.get(s + 1)), (Some(a), Some(b)) if a < b))
                .collect()
        };
        let (su, sv) = (spans(ku, du), spans(kv, dv));
        if su.is_empty() || sv.is_empty() {
            return Err("B-spline surface with an empty knot domain".into());
        }
        let mut ns = NurbsSurf {
            du,
            dv,
            nv,
            pw,
            ku: ku.to_vec(),
            kv: kv.to_vec(),
            su,
            sv,
            nodes: Vec::new(),
        };
        ns.build(0, ns.su.len() as u32, 0, ns.sv.len() as u32, 0)
            .ok_or_else(|| "B-spline surface: its span grid is too large".to_string())?;
        Ok(ns)
    }

    /// The number of nonempty knot spans.
    pub fn span_count(&self) -> usize {
        self.su.len() * self.sv.len()
    }

    /// Box of the control points acting on span (a, b).
    fn span_box(&self, a: usize, b: usize) -> Option<(V3, V3)> {
        let (s, t) = (*self.su.get(a)?, *self.sv.get(b)?);
        let mut lo = V3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = V3::new(f64::MIN, f64::MIN, f64::MIN);
        for i in s.checked_sub(self.du)?..=s {
            for j in t.checked_sub(self.dv)?..=t {
                let h = self.pw.get(i * self.nv + j)?;
                let q = V3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
                lo = V3::new(lo.x.min(q.x), lo.y.min(q.y), lo.z.min(q.z));
                hi = V3::new(hi.x.max(q.x), hi.y.max(q.y), hi.z.max(q.z));
            }
        }
        Some((lo, hi))
    }

    /// The quadtree node over spans [i0, i1) x [j0, j1) and its descendants, at
    /// `depth` below the root; None past `MAX_TREE_DEPTH`.
    fn build(&mut self, i0: u32, i1: u32, j0: u32, j1: u32, depth: u32) -> Option<u32> {
        if depth > MAX_TREE_DEPTH {
            return None;
        }
        let id = self.nodes.len() as u32;
        self.nodes.push(QNode {
            lo: V3::default(),
            hi: V3::default(),
            i0,
            j0,
            kids: Vec::new(),
        });
        let (lo, hi, kids) = if i1 - i0 == 1 && j1 - j0 == 1 {
            let (lo, hi) = self.span_box(i0 as usize, j0 as usize)?;
            (lo, hi, Vec::new())
        } else {
            let (im, jm) = ((i0 + i1) / 2, (j0 + j1) / 2);
            let mut kids = Vec::new();
            let iranges: Vec<(u32, u32)> = if i1 - i0 > 1 {
                vec![(i0, im), (im, i1)]
            } else {
                vec![(i0, i1)]
            };
            let jranges: Vec<(u32, u32)> = if j1 - j0 > 1 {
                vec![(j0, jm), (jm, j1)]
            } else {
                vec![(j0, j1)]
            };
            for &(a, b) in &iranges {
                for &(c, d) in &jranges {
                    kids.push(self.build(a, b, c, d, depth + 1)?);
                }
            }
            let mut lo = V3::new(f64::MAX, f64::MAX, f64::MAX);
            let mut hi = V3::new(f64::MIN, f64::MIN, f64::MIN);
            for &k in &kids {
                let n = self.nodes.get(k as usize)?;
                lo = V3::new(lo.x.min(n.lo.x), lo.y.min(n.lo.y), lo.z.min(n.lo.z));
                hi = V3::new(hi.x.max(n.hi.x), hi.y.max(n.hi.y), hi.z.max(n.hi.z));
            }
            (lo, hi, kids)
        };
        let n = self.nodes.get_mut(id as usize)?;
        n.lo = lo;
        n.hi = hi;
        n.kids = kids;
        Some(id)
    }

    /// The Bezier patch of span (a, b), by local knot insertion in u then v.
    fn patch(&self, a: usize, b: usize) -> Option<Patch> {
        let (s, t) = (*self.su.get(a)?, *self.sv.get(b)?);
        let (du, dv) = (self.du, self.dv);
        let (s0, t0) = (s.checked_sub(du)?, t.checked_sub(dv)?);
        let lu = self.ku.get(s0..=s + du + 1)?;
        let lv = self.kv.get(t0..=t + dv + 1)?;
        // Along u for each local column j, then along v for each Bezier row i.
        let cols: Vec<Vec<[f64; 4]>> = (t0..=t)
            .map(|j| {
                let col = (s0..=s)
                    .map(|i| self.pw.get(i * self.nv + j).copied())
                    .collect::<Option<Vec<_>>>()?;
                span_bezier(du, lu, &col)
            })
            .collect::<Option<_>>()?;
        let pw: Vec<Vec<[f64; 4]>> = (0..=du)
            .map(|i| {
                let row = cols
                    .iter()
                    .map(|c| c.get(i).copied())
                    .collect::<Option<Vec<_>>>()?;
                span_bezier(dv, lv, &row)
            })
            .collect::<Option<_>>()?;
        Some(Patch {
            pw,
            u0: *self.ku.get(s)?,
            u1: *self.ku.get(s + 1)?,
            v0: *self.kv.get(t)?,
            v1: *self.kv.get(t + 1)?,
        })
    }

    /// Index of the span holding parameter x (the last span for the far end).
    fn span_of(spans: &[usize], k: &[f64], x: f64) -> usize {
        spans
            .iter()
            .rposition(|&s| k.get(s).is_some_and(|&ks| ks <= x))
            .unwrap_or_default()
    }

    /// Point and derivatives at global parameters (u, v); derivatives per unit u and v.
    pub fn eval(&self, u: f64, v: f64) -> Option<SEval> {
        let (a, b) = (
            Self::span_of(&self.su, &self.ku, u),
            Self::span_of(&self.sv, &self.kv, v),
        );
        let pa = self.patch(a, b)?;
        let (s, t) = ((u - pa.u0) / (pa.u1 - pa.u0), (v - pa.v0) / (pa.v1 - pa.v0));
        let e = pa.eval(s.clamp(0.0, 1.0), t.clamp(0.0, 1.0));
        let (x, y) = (1.0 / (pa.u1 - pa.u0), 1.0 / (pa.v1 - pa.v0));
        Some(SEval {
            p: e.p,
            ps: e.ps * x,
            pt: e.pt * y,
            pss: e.pss * (x * x),
            pst: e.pst * (x * y),
            ptt: e.ptt * (y * y),
        })
    }

    /// Box holding the whole surface.
    fn bbox(&self) -> Option<(V3, V3)> {
        self.nodes.first().map(|n| (n.lo, n.hi))
    }

    /// Certified closest point of the untrimmed surface: branch and bound down the
    /// span quadtree (box bounds) into extracted patches (box and support-plane
    /// bounds, Newton at each popped patch).
    pub fn closest(&self, p: V3, cutoff: f64) -> Option<SurfHit> {
        let mut best: Option<SurfHit> = None;
        let mut best_d = cutoff;
        let mut heap = std::collections::BinaryHeap::new();
        heap.push(PatchItem {
            lb: box_dist(p, &self.bbox()?),
            entry: Entry::Node(0),
            depth: 0,
        });
        let mut pops = 0;
        while let Some(PatchItem { lb, entry, depth }) = heap.pop() {
            if lb >= best_d - DIST_EPS || best_d <= DIST_EPS {
                break;
            }
            pops += 1;
            if pops > 20000 {
                break;
            }
            let pa = match entry {
                Entry::Node(id) => {
                    let n = self.nodes.get(id as usize)?;
                    if n.kids.is_empty() {
                        let pa = self.patch(n.i0 as usize, n.j0 as usize)?;
                        let lbp = box_dist(p, &pa.bbox()).max(lb);
                        if lbp < best_d - DIST_EPS {
                            heap.push(PatchItem {
                                lb: lbp,
                                entry: Entry::Patch(pa),
                                depth: 0,
                            });
                        }
                    } else {
                        for &k in &n.kids {
                            let c = self.nodes.get(k as usize)?;
                            let lbk = box_dist(p, &(c.lo, c.hi));
                            if lbk < best_d - DIST_EPS {
                                heap.push(PatchItem {
                                    lb: lbk,
                                    entry: Entry::Node(k),
                                    depth: 0,
                                });
                            }
                        }
                    }
                    continue;
                }
                Entry::Patch(pa) => pa,
            };
            let (s, t) = pa.newton_capped(p, 0.5, 0.5, SEARCH_NEWTON);
            let e = pa.eval(s, t);
            let d = (e.p - p).len();
            let n = e.ps.cross(e.pt).unit();
            if d < best_d {
                best_d = d;
                best = Some(SurfHit {
                    q: e.p,
                    dist: d,
                    u: pa.u0 + s * (pa.u1 - pa.u0),
                    v: pa.v0 + t * (pa.v1 - pa.v0),
                    n,
                });
            }
            let slab = slab_bound(&pa, p, if d > 1e-12 { (p - e.p) * (1.0 / d) } else { n });
            if slab >= best_d - DIST_EPS || depth >= 40 {
                continue;
            }
            for q in pa.quarters() {
                let lbq = box_dist(p, &q.bbox()).max(slab);
                if lbq < best_d - DIST_EPS {
                    heap.push(PatchItem {
                        lb: lbq,
                        entry: Entry::Patch(q),
                        depth: depth + 1,
                    });
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame {
            o: V3::new(1.0, 2.0, 3.0),
            z: V3::new(0.0, 0.6, 0.8),
            x: V3::new(1.0, 0.0, 0.0),
        }
    }

    /// Every analytic foot is a critical point of the distance (p - q parallel to the
    /// normal) and lies on its surface.
    #[test]
    fn analytic_feet_are_critical_points() {
        let f = frame();
        let surfs = [
            Surf::Plane { f },
            Surf::Cylinder { f, r: 2.0 },
            Surf::Cone { f, r: 2.0, a: 0.5 },
            Surf::Sphere { f, r: 2.0 },
            Surf::Torus {
                f,
                major: 5.0,
                minor: 1.5,
            },
        ];
        for s in &surfs {
            for k in 0..200 {
                let x = f64::from(k);
                let p = V3::new(
                    1.0 + 7.0 * (x * 0.37).sin(),
                    2.0 + 6.0 * (x * 0.73).cos(),
                    3.0 + 5.0 * (x * 1.31).sin(),
                );
                let q = s.foot(p);
                let n = s.normal(q);
                let d = p - q;
                if let Surf::Cone { .. } = s {
                    if (q - s.apex().unwrap()).len() < 1e-9 {
                        continue;
                    }
                }
                assert!(
                    d.cross(n).len() < 1e-9 * (1.0 + d.len()),
                    "{}: p - q not normal at {p:?}",
                    s.name()
                );
                assert!(s.dist(q) < 1e-12, "{}: foot off the surface", s.name());
            }
        }
    }

    /// A bicubic patch surface: the certified closest point is no farther than any
    /// point of a dense parameter sweep.
    #[test]
    fn nurbs_closest_beats_dense_sampling() {
        let mut ctrl = Vec::new();
        for i in 0..5 {
            let mut row = Vec::new();
            for j in 0..4 {
                let (x, y) = (f64::from(i) * 2.0, f64::from(j) * 2.0);
                row.push(V3::new(x, y, (x * 0.7).sin() * 2.0 + (y * 0.9).cos()));
            }
            ctrl.push(row);
        }
        let ku = [0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0];
        let kv = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let s = NurbsSurf::new(3, 3, &ctrl, None, &ku, &kv).unwrap();
        assert_eq!(s.span_count(), 2);
        for k in 0..40 {
            let x = f64::from(k);
            let p = V3::new(
                4.0 + 5.0 * (x * 0.37).sin(),
                3.0 + 4.0 * (x * 0.71).cos(),
                1.0 + 3.0 * (x * 1.3).sin(),
            );
            let h = s.closest(p, f64::INFINITY).unwrap();
            let mut dense = f64::INFINITY;
            for a in 0..=200 {
                for b in 0..=200 {
                    let e = s.eval(f64::from(a) / 200.0, f64::from(b) / 200.0).unwrap();
                    dense = dense.min((e.p - p).len());
                }
            }
            assert!(
                h.dist <= dense + 1e-9,
                "closest {} vs dense {}",
                h.dist,
                dense
            );
            assert!(
                h.dist > dense - 0.05,
                "closest {} far below dense {}",
                h.dist,
                dense
            );
        }
    }
}
