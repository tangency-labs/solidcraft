//! Rational Bézier pieces: evaluation, halving, hulls, normal cones, and the
//! tests that part two pieces or prove their pair loop-free.

use std::cell::OnceCell;

/// A homogeneous point (w x, w y, w z, w).
pub type H = [f64; 4];
/// A point or direction in space.
pub type V3 = [f64; 3];

/// The highest degree a piece may have (the Bernstein scratch arrays' size).
pub const MAX_DEGREE: usize = 31;

/// a - b.
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
/// a + b.
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
/// The dot product.
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
/// The cross product.
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
/// The Euclidean length.
pub fn norm(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
/// a times s.
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
/// a scaled to length 1.
pub fn unit(a: V3) -> V3 {
    scale(a, 1.0 / norm(a))
}

/// The point h / w of a homogeneous point h.
fn project(h: H) -> V3 {
    [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
}

/// The derivative of the projection at `point` (= h / w, w = h[3]), from the
/// homogeneous derivative d.
fn projected_derivative(d: H, point: V3, w: f64) -> V3 {
    [
        (d[0] - point[0] * d[3]) / w,
        (d[1] - point[1] * d[3]) / w,
        (d[2] - point[2] * d[3]) / w,
    ]
}

/// Bernstein values and derivatives of degree n at t, written into `out` and
/// `dout`; nothing is written when they hold fewer than n + 1 values.
fn bernstein_into(n: usize, t: f64, out: &mut [f64], dout: &mut [f64]) {
    let (Some(out), Some(dout)) = (out.get_mut(..=n), dout.get_mut(..=n)) else {
        return;
    };
    let s = 1.0 - t;
    if let Some(first) = out.first_mut() {
        *first = 1.0;
    }
    if n == 0 {
        if let Some(first) = dout.first_mut() {
            *first = 0.0;
        }
    }
    for k in 1..=n {
        if k == n {
            // dout[i] = n (out[i - 1] - out[i]), out[-1] and out[n] taken as 0.
            let mut left = 0.0;
            for (i, d) in dout.iter_mut().enumerate() {
                let right = if i < n {
                    out.get(i).copied().unwrap_or(0.0)
                } else {
                    0.0
                };
                *d = n as f64 * (left - right);
                left = right;
            }
        }
        let mut saved = 0.0;
        for o in out.iter_mut().take(k) {
            let temp = *o;
            *o = saved + s * temp;
            saved = t * temp;
        }
        if let Some(o) = out.get_mut(k) {
            *o = saved;
        }
    }
}

/// The two halves at 1/2 of the control points of a Bézier curve (homogeneous):
/// de Casteljau on `work`, which holds the curve and is used up.
fn halve_into(work: &mut [H], mut left: impl FnMut(usize, H), mut right: impl FnMut(usize, H)) {
    let n = work.len().saturating_sub(1);
    for r in 0..=n {
        if let Some(&w) = work.first() {
            left(r, w);
        }
        if let Some(&w) = work.get(n - r) {
            right(n - r, w);
        }
        for i in 0..n - r {
            if let Some([a, b]) = work.get_mut(i..i + 2) {
                for (x, y) in a.iter_mut().zip(*b) {
                    *x = 0.5 * (*x + y);
                }
            }
        }
    }
}

/// A cone of directions: every unit vector within `angle` of `axis`.
#[derive(Clone, Copy, Debug)]
pub struct Cone {
    /// The unit axis.
    pub axis: V3,
    /// The half-angle (radians).
    pub angle: f64,
}

/// The cone holding every direction of positive combinations of `vectors`, or
/// None when they do not fit in an open half-space. Vectors shorter than 1e-12
/// of the longest are ignored: they are a collapsed edge's zero differences,
/// not directions.
pub fn cone_of(vectors: &[V3]) -> Option<Cone> {
    let longest = vectors.iter().map(|&v| norm(v)).fold(0.0, f64::max);
    let tiny = 1e-12 * longest;
    let mut units = Vec::with_capacity(vectors.len());
    let mut sum = [0.0; 3];
    for &v in vectors {
        let length = norm(v);
        if !(length > tiny) {
            continue;
        }
        let u = scale(v, 1.0 / length);
        sum = add(sum, u);
        units.push(u);
    }
    let length = norm(sum);
    if units.is_empty() || !(length > 1e-12 * units.len() as f64) {
        return None;
    }
    let axis = scale(sum, 1.0 / length);
    let least = units.iter().map(|&u| dot(u, axis)).fold(1.0, f64::min);
    if !(least > 1e-9) {
        return None;
    }
    Some(Cone {
        axis,
        angle: least.min(1.0).acos(),
    })
}

/// A point and its first derivatives on a piece.
#[derive(Clone, Copy, Debug)]
pub struct Eval {
    /// The point.
    pub point: V3,
    /// The derivative in s.
    pub ds: V3,
    /// The derivative in t.
    pub dt: V3,
}

/// A rational Bézier piece of patch `patch` over [u0, u1] × [v0, v1], degrees
/// (du, dv), homogeneous coefficients `c` with (i, j) at i * (dv + 1) + j.
/// `above[d]` says the surface goes on past the piece's high end in u (d = 0)
/// or v (d = 1): another piece of the patch lies there, or the patch closes
/// on itself (a seam). Its projected points, box, normal cone and halves are
/// computed once.
#[derive(Clone)]
pub struct Piece {
    /// The index of the patch the piece is cut from.
    pub patch: usize,
    /// The low end of the piece's knot interval in u.
    pub u0: f64,
    /// The high end in u.
    pub u1: f64,
    /// The low end in v.
    pub v0: f64,
    /// The high end in v.
    pub v1: f64,
    /// The degree in u.
    pub du: usize,
    /// The degree in v.
    pub dv: usize,
    /// The (du + 1) (dv + 1) homogeneous coefficients, (i, j) at i * (dv + 1) + j.
    pub c: Vec<H>,
    /// Whether the surface goes on past the high end in u and in v.
    pub above: [bool; 2],
    points: OnceCell<Vec<V3>>,
    bounds: OnceCell<[f64; 6]>,
    cone: OnceCell<Option<Cone>>,
    halves: [OnceCell<Box<[Piece; 2]>>; 2],
}

impl Piece {
    /// The piece of `patch` over [u0, u1] × [v0, v1] with degrees (du, dv) and
    /// coefficients `c`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        patch: usize,
        u0: f64,
        u1: f64,
        v0: f64,
        v1: f64,
        du: usize,
        dv: usize,
        c: Vec<H>,
        above: [bool; 2],
    ) -> Piece {
        Piece {
            patch,
            u0,
            u1,
            v0,
            v1,
            du,
            dv,
            c,
            above,
            points: OnceCell::new(),
            bounds: OnceCell::new(),
            cone: OnceCell::new(),
            halves: [OnceCell::new(), OnceCell::new()],
        }
    }

    /// The projected control points.
    pub fn points(&self) -> &[V3] {
        self.points
            .get_or_init(|| self.c.iter().map(|&h| project(h)).collect())
    }

    /// [min x, min y, min z, max x, max y, max z] of the control points, which hold the piece (positive weights).
    pub fn bounds(&self) -> &[f64; 6] {
        self.bounds.get_or_init(|| box_of(self.points()))
    }

    /// The diagonal of the control points' box.
    pub fn size(&self) -> f64 {
        let b = self.bounds();
        norm([b[3] - b[0], b[4] - b[1], b[5] - b[2]])
    }

    /// A cone holding every normal Su × Sv of the piece, or None when none
    /// narrower than a half-space is known. Su is a positive multiple of a
    /// polynomial whose Bernstein coefficients bound its directions: for a
    /// polynomial piece the u differences of the control points, and for a
    /// rational one the numerator w Hu - wu H (degree 2du - 1 by 2dv), whose
    /// coefficients come from the Bernstein product rule. Su × Sv is bilinear,
    /// so the normals lie in the cone of the cross products of the two sets.
    pub fn cone(&self) -> Option<Cone> {
        *self.cone.get_or_init(|| {
            let w0 = self.c.first().map_or(f64::NAN, |h| h[3]);
            let rational = self.c.iter().any(|h| (h[3] - w0).abs() > 1e-12 * w0.abs());
            let (gu, gv) = if rational {
                (derivative_numerator(self, 0), derivative_numerator(self, 1))
            } else {
                (differences(self, 0), differences(self, 1))
            };
            let mut normals = Vec::with_capacity(gu.len() * gv.len());
            for &a in &gu {
                for &b in &gv {
                    normals.push(cross(a, b));
                }
            }
            cone_of(&normals)
        })
    }

    /// The control polygon's longest run along u (dir 0) or v (dir 1).
    pub fn extent(&self, dir: usize) -> f64 {
        let row_length = self.dv + 1;
        let p = self.points();
        let run = |line: &mut dyn Iterator<Item = &V3>| {
            let mut run = 0.0;
            let mut last = line.next();
            for y in line {
                if let Some(x) = last {
                    run += norm(sub(*y, *x));
                }
                last = Some(y);
            }
            run
        };
        let mut longest: f64 = 0.0;
        if dir == 0 {
            for j in 0..row_length {
                longest = longest.max(run(&mut p.iter().skip(j).step_by(row_length)));
            }
        } else {
            for row in p.chunks(row_length) {
                longest = longest.max(run(&mut row.iter()));
            }
        }
        longest
    }

    /// The two halves along u (dir 0) or v (dir 1), by de Casteljau at 1/2; computed once.
    pub fn halves(&self, dir: usize) -> &[Piece; 2] {
        let cell = if dir == 0 {
            &self.halves[0]
        } else {
            &self.halves[1]
        };
        cell.get_or_init(|| {
            let (du, dv) = (self.du, self.dv);
            let (n, m) = if dir == 0 { (du, dv) } else { (dv, du) };
            let index = |i: usize, j: usize| {
                if dir == 0 {
                    i * (dv + 1) + j
                } else {
                    j * (dv + 1) + i
                }
            };
            let mut left = vec![[0.0; 4]; self.c.len()];
            let mut right = vec![[0.0; 4]; self.c.len()];
            let mut work = vec![[0.0; 4]; n + 1];
            for j in 0..=m {
                for (i, w) in work.iter_mut().enumerate() {
                    if let Some(&c) = self.c.get(index(i, j)) {
                        *w = c;
                    }
                }
                halve_into(
                    &mut work,
                    |r, w| {
                        if let Some(x) = left.get_mut(index(r, j)) {
                            *x = w;
                        }
                    },
                    |r, w| {
                        if let Some(x) = right.get_mut(index(r, j)) {
                            *x = w;
                        }
                    },
                );
            }
            let (u0, u1, v0, v1) = (self.u0, self.u1, self.v0, self.v1);
            // The low half always has the high half above it.
            let mut low_above = self.above;
            if let Some(a) = low_above.get_mut(dir) {
                *a = true;
            }
            let halves = if dir == 0 {
                let um = (u0 + u1) / 2.0;
                [
                    Piece::new(self.patch, u0, um, v0, v1, du, dv, left, low_above),
                    Piece::new(self.patch, um, u1, v0, v1, du, dv, right, self.above),
                ]
            } else {
                let vm = (v0 + v1) / 2.0;
                [
                    Piece::new(self.patch, u0, u1, v0, vm, du, dv, left, low_above),
                    Piece::new(self.patch, u0, u1, vm, v1, du, dv, right, self.above),
                ]
            };
            Box::new(halves)
        })
    }

    /// The point and first derivatives at local (s, t) in [0, 1]^2.
    pub fn evaluate(&self, s: f64, t: f64) -> Eval {
        let (du, dv) = (self.du, self.dv);
        let (mut bu, mut dbu, mut bv, mut dbv) = (
            [0.0; MAX_DEGREE + 1],
            [0.0; MAX_DEGREE + 1],
            [0.0; MAX_DEGREE + 1],
            [0.0; MAX_DEGREE + 1],
        );
        bernstein_into(du, s, &mut bu, &mut dbu);
        bernstein_into(dv, t, &mut bv, &mut dbv);
        let (mut h, mut hs, mut ht) = ([0.0; 4], [0.0; 4], [0.0; 4]);
        for (row, (&bi, &dbi)) in self.c.chunks(dv + 1).zip(bu.iter().zip(&dbu)).take(du + 1) {
            for (c, (&bj, &dbj)) in row.iter().zip(bv.iter().zip(&dbv)) {
                let (w, ws, wt) = (bi * bj, dbi * bj, bi * dbj);
                for (((hk, hsk), htk), ck) in h.iter_mut().zip(&mut hs).zip(&mut ht).zip(c) {
                    *hk += w * ck;
                    *hsk += ws * ck;
                    *htk += wt * ck;
                }
            }
        }
        let w = h[3];
        let point = [h[0] / w, h[1] / w, h[2] / w];
        let ds = projected_derivative(hs, point, w);
        let dt = projected_derivative(ht, point, w);
        Eval { point, ds, dt }
    }

    /// Local (s, t) in this piece of a point at local (s, t) of `other`, a piece inside this one.
    pub fn local_of(&self, other: &Piece, s: f64, t: f64) -> (f64, f64) {
        let u = other.u0 + s * (other.u1 - other.u0);
        let v = other.v0 + t * (other.v1 - other.v0);
        (
            (u - self.u0) / (self.u1 - self.u0),
            (v - self.v0) / (self.v1 - self.v0),
        )
    }

    /// The boundary curve: s = 0, s = 1, t = 0 or t = 1 (edges 0 to 3), as homogeneous points along the other parameter.
    pub fn edge(&self, e: usize) -> Vec<H> {
        let (du, dv) = (self.du, self.dv);
        let count = if e < 2 { dv + 1 } else { du + 1 };
        (0..count)
            .filter_map(|k| {
                let (i, j) = match e {
                    0 => (0, k),
                    1 => (du, k),
                    2 => (k, 0),
                    _ => (k, dv),
                };
                self.c.get(i * (dv + 1) + j).copied()
            })
            .collect()
    }
}

/// [min x, min y, min z, max x, max y, max z] of the points.
pub fn box_of(points: &[V3]) -> [f64; 6] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let (lo, hi) = b.split_at_mut(3);
    for p in points {
        for ((l, h), &x) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
            *l = l.min(x);
            *h = h.max(x);
        }
    }
    b
}

/// C(n, k), exact in f64 for the degrees a piece can have.
fn binomial(n: usize, k: usize) -> f64 {
    let mut r = 1.0;
    for i in 1..=k {
        r = r * (n - k + i) as f64 / i as f64;
    }
    r
}

/// A polynomial piece's control-point differences along u (dir 0) or v (dir 1): Su or Sv's Bernstein directions.
fn differences(piece: &Piece, dir: usize) -> Vec<V3> {
    let rows = piece.points().chunks(piece.dv + 1);
    let mut out = Vec::new();
    if dir == 0 {
        for (r0, r1) in rows.clone().zip(rows.skip(1)) {
            for (x, y) in r0.iter().zip(r1) {
                out.push(sub(*y, *x));
            }
        }
    } else {
        for row in rows {
            for w in row.windows(2) {
                if let [x, y] = w {
                    out.push(sub(*y, *x));
                }
            }
        }
    }
    out
}

/// A rational piece's w H' - w' H along u (dir 0) or v (dir 1), as Bernstein
/// coefficients: a positive multiple of Su or Sv everywhere.
fn derivative_numerator(piece: &Piece, dir: usize) -> Vec<V3> {
    let (du, dv) = (piece.du, piece.dv);
    let h = |i: usize, j: usize| {
        piece
            .c
            .get(i * (dv + 1) + j)
            .copied()
            .unwrap_or([f64::NAN; 4])
    };
    // The difference net and the degrees of the two factors in each direction.
    let (ku, kv) = if dir == 0 {
        (du.saturating_sub(1), dv)
    } else {
        (du, dv.saturating_sub(1))
    };
    let d = |k: usize, l: usize| {
        let (a, b) = if dir == 0 {
            (h(k + 1, l), h(k, l))
        } else {
            (h(k, l + 1), h(k, l))
        };
        [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
    };
    let (nu, nv) = (du + ku, dv + kv);
    let mut out = vec![[0.0; 3]; (nu + 1) * (nv + 1)];
    for i in 0..=du {
        for j in 0..=dv {
            let hij = h(i, j);
            for k in 0..=ku {
                for l in 0..=kv {
                    let dkl = d(k, l);
                    let alpha = (binomial(du, i) * binomial(ku, k) / binomial(nu, i + k))
                        * (binomial(dv, j) * binomial(kv, l) / binomial(nv, j + l));
                    if let Some(o) = out.get_mut((i + k) * (nv + 1) + j + l) {
                        for ((om, &dm), &hm) in o.iter_mut().zip(&dkl).zip(&hij) {
                            *om += alpha * (hij[3] * dm - dkl[3] * hm);
                        }
                    }
                }
            }
        }
    }
    out
}

/// A rational Bézier curve (homogeneous points) at t: point and derivative.
pub fn curve_at(c: &[H], t: f64) -> (V3, V3) {
    let n = c.len().saturating_sub(1);
    let (mut b, mut db) = ([0.0; MAX_DEGREE + 1], [0.0; MAX_DEGREE + 1]);
    bernstein_into(n, t, &mut b, &mut db);
    let (mut h, mut hd) = ([0.0; 4], [0.0; 4]);
    for (ci, (&bi, &dbi)) in c.iter().zip(b.iter().zip(&db)) {
        for ((hk, hdk), &ck) in h.iter_mut().zip(&mut hd).zip(ci) {
            *hk += bi * ck;
            *hdk += dbi * ck;
        }
    }
    let w = h[3];
    let point = [h[0] / w, h[1] / w, h[2] / w];
    (point, projected_derivative(hd, point, w))
}

/// The two halves of a homogeneous curve at 1/2.
pub fn split_curve(c: &[H]) -> (Vec<H>, Vec<H>) {
    let mut left = vec![[0.0; 4]; c.len()];
    let mut right = vec![[0.0; 4]; c.len()];
    let mut work = c.to_vec();
    halve_into(
        &mut work,
        |r, w| {
            if let Some(x) = left.get_mut(r) {
                *x = w;
            }
        },
        |r, w| {
            if let Some(x) = right.get_mut(r) {
                *x = w;
            }
        },
    );
    (left, right)
}

/// Projected points, box, size and tangent cone of a homogeneous curve.
pub struct Hull {
    /// The projected control points.
    pub points: Vec<V3>,
    /// Their box, as [`box_of`] gives it.
    pub bounds: [f64; 6],
    /// The box's diagonal.
    pub size: f64,
    /// A cone holding every tangent, when one narrower than a half-space is known.
    pub cone: Option<Cone>,
}

/// The hull of a homogeneous curve's control points.
pub fn curve_hull(c: &[H]) -> Hull {
    let points: Vec<V3> = c.iter().map(|&h| project(h)).collect();
    let bounds = box_of(&points);
    let differences: Vec<V3> = points
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some(sub(*b, *a)),
            _ => None,
        })
        .collect();
    let size = norm([
        bounds[3] - bounds[0],
        bounds[4] - bounds[1],
        bounds[5] - bounds[2],
    ]);
    Hull {
        cone: cone_of(&differences),
        points,
        bounds,
        size,
    }
}

/// Whether two boxes ([`box_of`]) are more than `pad` apart along an axis.
pub fn boxes_apart(a: &[f64; 6], b: &[f64; 6], pad: f64) -> bool {
    a[0] > b[3] + pad
        || b[0] > a[3] + pad
        || a[1] > b[4] + pad
        || b[1] > a[4] + pad
        || a[2] > b[5] + pad
        || b[2] > a[5] + pad
}

/// The range of `points` along `axis`.
fn range_along(points: &[V3], axis: V3) -> (f64, f64) {
    points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), &p| {
            let d = dot(p, axis);
            (low.min(d), high.max(d))
        })
}

/// Whether a slab across `axis` separates two point sets.
pub fn slab_apart(a: &[V3], b: &[V3], axis: V3, pad: f64) -> bool {
    let (a0, a1) = range_along(a, axis);
    let (b0, b1) = range_along(b, axis);
    a1 + pad < b0 || b1 + pad < a0
}

/// Whether two pieces are proved more than `pad` apart: by their boxes, or by
/// a slab across either one's normal axis.
pub fn pieces_apart(a: &Piece, b: &Piece, pad: f64) -> bool {
    if boxes_apart(a.bounds(), b.bounds(), pad) {
        return true;
    }
    // Slabs across each piece's own normal axis, tight for a nearly flat piece.
    for piece in [a, b] {
        if let Some(cone) = piece.cone() {
            if slab_apart(a.points(), b.points(), cone.axis, pad) {
                return true;
            }
        }
    }
    false
}

/// No normal of `a` is parallel to a normal of `b`, so no curve closes in the
/// pair and every crossing is transversal (Sederberg and Meyers's loop test).
pub fn loop_free(a: &Piece, b: &Piece) -> bool {
    let (Some(ca), Some(cb)) = (a.cone(), b.cone()) else {
        return false;
    };
    let theta = dot(ca.axis, cb.axis).clamp(-1.0, 1.0).acos();
    let spread = ca.angle + cb.angle + 1e-9;
    theta > spread && theta < std::f64::consts::PI - spread
}
