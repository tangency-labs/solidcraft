//! Plane geometry the gear outlines are made of: points, segments (lines, arcs, fit point
//! splines) and closed outlines, with sampling for previews.

use std::f64::consts::{PI, TAU};
use std::ops::{Add, Mul, Neg, Sub};

/// A point or vector in the plane, millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct V2 {
    pub x: f64,
    pub y: f64,
}

impl V2 {
    pub const ZERO: V2 = V2 { x: 0.0, y: 0.0 };
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    /// The point at radius `r` and polar angle `a` (radians, counter-clockwise from +x).
    pub fn polar(r: f64, a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Self { x: r * c, y: r * s }
    }
    pub fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }
    pub fn dot(self, o: V2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn cross(self, o: V2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn dist(self, o: V2) -> f64 {
        (self - o).len()
    }
    /// Turned counter-clockwise by `a` radians about the origin.
    pub fn rot(self, a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Self { x: self.x * c - self.y * s, y: self.x * s + self.y * c }
    }
    pub fn norm(self) -> Self {
        let l = self.len();
        if l > 0.0 { self * (1.0 / l) } else { self }
    }
    /// Mirrored across the x axis.
    pub fn flip_y(self) -> Self {
        Self { x: self.x, y: -self.y }
    }
    pub fn lerp(self, o: V2, t: f64) -> Self {
        self + (o - self) * t
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for V2 {
    type Output = V2;
    fn mul(self, k: f64) -> V2 {
        V2::new(self.x * k, self.y * k)
    }
}
impl Neg for V2 {
    type Output = V2;
    fn neg(self) -> V2 {
        V2::new(-self.x, -self.y)
    }
}

/// One piece of an outline, in the direction the outline runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Line(V2, V2),
    /// An arc about `c` from `a` to `b`: counter-clockwise when `ccw`, else clockwise.
    Arc {
        c: V2,
        a: V2,
        b: V2,
        ccw: bool,
    },
    /// A fit point spline through the points, first to last.
    Spline(Vec<V2>),
    /// A whole circle (only as an outline of its own).
    Circle(V2, f64),
}

impl Seg {
    pub fn start(&self) -> V2 {
        match self {
            Seg::Line(a, _) => *a,
            Seg::Arc { a, .. } => *a,
            Seg::Spline(p) => p[0],
            Seg::Circle(c, r) => *c + V2::new(*r, 0.0),
        }
    }
    pub fn end(&self) -> V2 {
        match self {
            Seg::Line(_, b) => *b,
            Seg::Arc { b, .. } => *b,
            Seg::Spline(p) => p[p.len() - 1],
            Seg::Circle(c, r) => *c + V2::new(*r, 0.0),
        }
    }
    /// Points along the segment, from its start to its end, `tol` the most a chord may stray from
    /// an arc. Splines are sampled as the sketch's fit point spline runs (see [`natural_spline`]).
    pub fn sample(&self, tol: f64, out: &mut Vec<V2>) {
        match self {
            Seg::Line(a, b) => {
                out.push(*a);
                out.push(*b);
            }
            Seg::Arc { c, a, b, ccw } => {
                let r = c.dist(*a);
                let a0 = (*a - *c).angle();
                let mut sweep = (*b - *c).angle() - a0;
                if *ccw {
                    while sweep <= 0.0 {
                        sweep += TAU;
                    }
                } else {
                    while sweep >= 0.0 {
                        sweep -= TAU;
                    }
                }
                let n = arc_steps(r, sweep.abs(), tol);
                for i in 0..=n {
                    let t = a0 + sweep * i as f64 / n as f64;
                    out.push(*c + V2::polar(r, t));
                }
            }
            Seg::Spline(p) => natural_spline(p, 6, out),
            Seg::Circle(c, r) => {
                let n = arc_steps(*r, TAU, tol);
                for i in 0..=n {
                    out.push(*c + V2::polar(*r, TAU * i as f64 / n as f64));
                }
            }
        }
    }
    /// The segment moved by `f`, which must be a rigid motion or a uniform scale (arcs stay arcs);
    /// `mirror` says whether `f` reverses orientation.
    pub fn map(&self, f: &dyn Fn(V2) -> V2, mirror: bool) -> Seg {
        match self {
            Seg::Line(a, b) => Seg::Line(f(*a), f(*b)),
            Seg::Arc { c, a, b, ccw } => Seg::Arc { c: f(*c), a: f(*a), b: f(*b), ccw: *ccw != mirror },
            Seg::Spline(p) => Seg::Spline(p.iter().map(|q| f(*q)).collect()),
            Seg::Circle(c, r) => {
                let k = f(*c + V2::new(*r, 0.0)).dist(f(*c));
                Seg::Circle(f(*c), k)
            }
        }
    }
    /// The same piece run the other way.
    pub fn reversed(&self) -> Seg {
        match self {
            Seg::Line(a, b) => Seg::Line(*b, *a),
            Seg::Arc { c, a, b, ccw } => Seg::Arc { c: *c, a: *b, b: *a, ccw: !*ccw },
            Seg::Spline(p) => Seg::Spline(p.iter().rev().copied().collect()),
            Seg::Circle(c, r) => Seg::Circle(*c, *r),
        }
    }
}

fn arc_steps(r: f64, sweep: f64, tol: f64) -> usize {
    let tol = tol.max(1e-6);
    let step = if r > tol { 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos() } else { PI };
    ((sweep / step.max(1e-3)).ceil() as usize).clamp(1, 720)
}

/// The curve a sketch's fit point spline makes through `p`: a natural cubic (zero curvature at
/// both ends) with chord-length parameters, as the sketch crate interpolates it; `n` points per
/// span, from `p[0]` on.
pub fn natural_spline(p: &[V2], n: usize, out: &mut Vec<V2>) {
    let k = p.len();
    if k < 3 {
        out.extend_from_slice(p);
        return;
    }
    let h: Vec<f64> = p.windows(2).map(|w| w[0].dist(w[1]).max(1e-12)).collect();
    // Second derivatives at the inner points (tridiagonal, Thomas algorithm).
    let m = k - 2;
    let mut diag = vec![0.0; m];
    let mut rhs = vec![V2::ZERO; m];
    for i in 0..m {
        diag[i] = 2.0 * (h[i] + h[i + 1]);
        rhs[i] = ((p[i + 2] - p[i + 1]) * (1.0 / h[i + 1]) - (p[i + 1] - p[i]) * (1.0 / h[i])) * 6.0;
    }
    for i in 1..m {
        let w = h[i] / diag[i - 1];
        diag[i] -= w * h[i];
        rhs[i] = rhs[i] - rhs[i - 1] * w;
    }
    let mut sol = vec![V2::ZERO; m];
    for i in (0..m).rev() {
        let next = if i + 1 < m { sol[i + 1] * h[i + 1] } else { V2::ZERO };
        sol[i] = (rhs[i] - next) * (1.0 / diag[i]);
    }
    let second = |i: usize| if i == 0 || i == k - 1 { V2::ZERO } else { sol[i - 1] };
    out.push(p[0]);
    for i in 0..k - 1 {
        let hi = h[i];
        let slope = (p[i + 1] - p[i]) * (1.0 / hi);
        let d0 = (slope - (second(i) * 2.0 + second(i + 1)) * (hi / 6.0)) * hi;
        let d1 = (slope + (second(i) + second(i + 1) * 2.0) * (hi / 6.0)) * hi;
        for j in 1..=n {
            let s = j as f64 / n as f64;
            let (s2, s3) = (s * s, s * s * s);
            out.push(p[i] * (2.0 * s3 - 3.0 * s2 + 1.0) + d0 * (s3 - 2.0 * s2 + s) + p[i + 1] * (3.0 * s2 - 2.0 * s3) + d1 * (s3 - s2));
        }
    }
}

/// Fit points for one side of a tooth given as its half angle against radius, `half(r)`, with
/// `grade` taking 0..1 to the radii it runs over (bunched where the side bends hardest): as few
/// as keep the sketch's spline through them ([`natural_spline`]) within `tol` of the side, and
/// with no turn of more than 30° between neighbouring chords. (radius, half angle) pairs, from
/// `grade(0)` to `grade(1)`.
pub fn fit_profile(half: &dyn Fn(f64) -> f64, grade: &dyn Fn(f64) -> f64, tol: f64, min: usize, max: usize) -> Vec<(f64, f64)> {
    const DENSE: usize = 128;
    let at = |s: f64| -> (f64, f64) {
        let r = grade(s);
        (r, half(r))
    };
    let dense: Vec<(f64, f64)> = (0..=DENSE).map(|i| at(i as f64 / DENSE as f64)).collect();
    let pts: Vec<V2> = dense.iter().map(|(r, h)| V2::polar(*r, *h)).collect();
    // A measure along the side, half its length and half its turning, so points crowd where it
    // bends.
    let mut length = vec![0.0; DENSE + 1];
    let mut turning = vec![0.0; DENSE + 1];
    for i in 1..=DENSE {
        length[i] = length[i - 1] + pts[i].dist(pts[i - 1]);
        let turn = if i >= 2 {
            let (u, v) = (pts[i - 1] - pts[i - 2], pts[i] - pts[i - 1]);
            u.cross(v).atan2(u.dot(v)).abs()
        } else {
            0.0
        };
        turning[i] = turning[i - 1] + turn;
    }
    let (total_l, total_t) = (length[DENSE].max(1e-300), turning[DENSE]);
    let measure: Vec<f64> =
        (0..=DENSE).map(|i| if total_t > 1e-9 { 0.5 * length[i] / total_l + 0.5 * turning[i] / total_t } else { length[i] / total_l }).collect();
    // The dense parameter s at which the measure reaches u.
    let s_at = |u: f64| -> f64 {
        let i = measure.partition_point(|m| *m < u).clamp(1, DENSE);
        let (m0, m1) = (measure[i - 1], measure[i]);
        let f = if m1 > m0 { ((u - m0) / (m1 - m0)).clamp(0.0, 1.0) } else { 0.0 };
        (i as f64 - 1.0 + f) / DENSE as f64
    };
    let mut best = Vec::new();
    for n in min.max(1)..=max.max(min.max(1)) {
        let ss: Vec<f64> = (0..=n)
            .map(|j| {
                let t = j as f64 / n as f64;
                // Somewhat closer together at the ends, where the spline's free ends sag most.
                let u = 0.6 * t + 0.4 * (1.0 - (PI * t).cos()) / 2.0;
                if j == 0 {
                    0.0
                } else if j == n {
                    1.0
                } else {
                    s_at(u)
                }
            })
            .collect();
        // Between the dense samples by a straight line while searching (they are close enough
        // to judge the count by); the points returned are exact.
        let between = |s: f64| -> (f64, f64) {
            let x = (s * DENSE as f64).clamp(0.0, DENSE as f64);
            let i = (x.floor() as usize).min(DENSE - 1);
            let f = x - i as f64;
            (dense[i].0 + (dense[i + 1].0 - dense[i].0) * f, dense[i].1 + (dense[i + 1].1 - dense[i].1) * f)
        };
        let fit: Vec<(f64, f64)> = ss.iter().map(|s| between(*s)).collect();
        let q: Vec<V2> = fit.iter().map(|(r, h)| V2::polar(*r, *h)).collect();
        let mut sharpest: f64 = 0.0;
        for w in q.windows(3) {
            let (u, v) = (w[1] - w[0], w[2] - w[1]);
            sharpest = sharpest.max(u.cross(v).atan2(u.dot(v)).abs());
        }
        let mut curve = Vec::new();
        if q.len() < 3 {
            curve.extend((0..=8).map(|i| q[0].lerp(q[1], i as f64 / 8.0)));
        } else {
            natural_spline(&q, 8, &mut curve);
        }
        let mut worst: f64 = 0.0;
        for j in 0..n {
            let lo = ((ss[j] * DENSE as f64).floor() as usize).saturating_sub(2);
            let hi = ((ss[j + 1] * DENSE as f64).ceil() as usize + 2).min(DENSE);
            for c in &curve[j * 8..=(j + 1) * 8] {
                let mut d = f64::INFINITY;
                for k in lo..hi {
                    let (a, b) = (pts[k], pts[k + 1]);
                    let e = b - a;
                    let l2 = e.dot(e);
                    let t = if l2 > 0.0 { ((*c - a).dot(e) / l2).clamp(0.0, 1.0) } else { 0.0 };
                    d = d.min(c.dist(a + e * t));
                }
                worst = worst.max(d);
            }
        }
        let done = worst <= tol && sharpest <= 30f64.to_radians();
        if done || n >= max.max(min.max(1)) {
            best = ss
                .iter()
                .map(|s| {
                    if *s == 0.0 {
                        dense[0]
                    } else if *s == 1.0 {
                        dense[DENSE]
                    } else {
                        at(*s)
                    }
                })
                .collect();
            break;
        }
    }
    best
}

/// A closed outline: segments end to end, the last ending where the first starts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outline {
    pub segs: Vec<Seg>,
}

impl Outline {
    pub fn circle(c: V2, r: f64) -> Self {
        Self { segs: vec![Seg::Circle(c, r)] }
    }
    /// Points round the outline (closed: the last equals the first).
    pub fn polyline(&self, tol: f64) -> Vec<V2> {
        let mut out = Vec::new();
        for s in &self.segs {
            let mut pts = Vec::new();
            s.sample(tol, &mut pts);
            if !out.is_empty() && !pts.is_empty() {
                pts.remove(0);
            }
            out.extend(pts);
        }
        out
    }
    pub fn map(&self, f: &dyn Fn(V2) -> V2, mirror: bool) -> Outline {
        let mut segs: Vec<Seg> = self.segs.iter().map(|s| s.map(f, mirror)).collect();
        if mirror {
            segs = segs.iter().rev().map(Seg::reversed).collect();
        }
        Outline { segs }
    }
    pub fn rotated(&self, a: f64) -> Outline {
        self.map(&|p| p.rot(a), false)
    }
    pub fn translated(&self, d: V2) -> Outline {
        self.map(&|p| p + d, false)
    }
    pub fn scaled(&self, k: f64) -> Outline {
        self.map(&|p| p * k, false)
    }
    /// The largest distance from the origin among the outline's sample points.
    pub fn max_radius(&self) -> f64 {
        self.polyline(0.05).iter().map(|p| p.len()).fold(0.0, f64::max)
    }
    /// Signed area (positive counter-clockwise) of the sampled outline.
    pub fn area(&self) -> f64 {
        let p = self.polyline(0.01);
        let mut a = 0.0;
        for w in p.windows(2) {
            a += w[0].cross(w[1]);
        }
        a * 0.5
    }
}

/// A planar region: an outer outline and the holes in it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region {
    pub outer: Vec<Outline>,
    pub holes: Vec<Outline>,
}

/// The involute function: tan a − a.
pub fn inv(a: f64) -> f64 {
    a.tan() - a
}

/// The angle whose involute function is `v` (0 ≤ v < ~2), by Newton's method.
pub fn inv_inv(v: f64) -> f64 {
    if v <= 0.0 {
        return 0.0;
    }
    // Cheng's starting guess, then Newton on tan a − a − v (derivative tan² a).
    let mut a = (3.0 * v).cbrt() - 0.2 * v.powf(1.0 / 3.0) * v.powf(2.0 / 3.0).min(1.0);
    a = a.clamp(1e-6, 1.5);
    for _ in 0..50 {
        let t = a.tan();
        let f = t - a - v;
        let d = t * t;
        if d <= 0.0 {
            break;
        }
        let next = (a - f / d).clamp(1e-9, std::f64::consts::FRAC_PI_2);
        if (next - a).abs() < 1e-15 {
            a = next;
            break;
        }
        a = next;
    }
    a
}

/// Wraps an angle into (−π, π].
pub fn wrap(a: f64) -> f64 {
    let mut a = a % TAU;
    if a > PI {
        a -= TAU;
    } else if a <= -PI {
        a += TAU;
    }
    a
}

/// Wraps `a` into (−period/2, period/2].
pub fn wrap_period(a: f64, period: f64) -> f64 {
    let mut a = a % period;
    if a > period / 2.0 {
        a -= period;
    } else if a <= -period / 2.0 {
        a += period;
    }
    a
}
