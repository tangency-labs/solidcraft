//! Exact distances to the corpus truths and B-spline/NURBS evaluation: a port of the parts of
//! the corpus's `scoring/ssi_geom.py` the scorer uses (`truth_distance`, `param_distance`,
//! `bspline_basis`, `Nurbs.eval`). Arithmetic follows numpy's order of operations so the
//! numbers match the Python scorer to the last few bits.

use serde_json::Value;

pub type V3 = [f64; 3];

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: V3) -> V3 {
    let n = norm(a);
    [a[0] / n, a[1] / n, a[2] / n]
}

/// numpy's `x ** i` for a float array and a small integer exponent.
pub fn powi_np(x: f64, i: usize) -> f64 {
    match i {
        0 => 1.0,
        1 => x,
        2 => x * x,
        _ => x.powf(i as f64),
    }
}

/// numpy's `np.clip` (NaN propagates).
pub fn clip(x: f64, lo: f64, hi: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    let y = if x < lo { lo } else { x };
    if y > hi { hi } else { y }
}

/// numpy's `ndarray.max()`: NaN wins; `-inf` for an empty slice (numpy raises, never reached).
pub fn np_max(xs: &[f64]) -> f64 {
    let mut m = f64::NEG_INFINITY;
    for &x in xs {
        if x.is_nan() {
            return x;
        }
        if x > m {
            m = x;
        }
    }
    m
}

/// numpy's `np.minimum` on two scalars (NaN wins).
pub fn np_minimum(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if b < a {
        b
    } else {
        a
    }
}

/// Python's builtin `max(a, b)`: keeps `a` unless `b > a`.
pub fn py_max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// Python's builtin `min(a, b)`: keeps `a` unless `b < a`.
pub fn py_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// numpy's `linspace(start, stop, num)`.
pub fn linspace(start: f64, stop: f64, num: usize) -> Vec<f64> {
    if num == 0 {
        return Vec::new();
    }
    if num == 1 {
        return vec![start];
    }
    let step = (stop - start) / (num - 1) as f64;
    let mut out: Vec<f64> = (0..num).map(|i| i as f64 * step + start).collect();
    if let Some(last) = out.last_mut() {
        *last = stop;
    }
    out
}

// ------------------------------------------------------------------------------------------
// JSON helpers

pub fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

pub fn v3(v: &Value) -> Result<V3, String> {
    match v {
        Value::Null => Ok([f64::NAN; 3]),
        Value::Array(a) if a.len() == 3 => Ok([num(&a[0]), num(&a[1]), num(&a[2])]),
        _ => Err(format!("not a 3-vector: {v}")),
    }
}

pub fn v3_list(v: Option<&Value>) -> Result<Vec<V3>, String> {
    match v {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a.iter().map(v3).collect(),
        Some(other) => Err(format!("not a point list: {other}")),
    }
}

fn field<'a>(v: &'a Value, key: &str) -> Result<&'a Value, String> {
    v.get(key).ok_or_else(|| format!("missing field {key}"))
}

fn f64_field(v: &Value, key: &str) -> Result<f64, String> {
    field(v, key)?.as_f64().ok_or_else(|| format!("{key} is not a number"))
}

fn f64_list(v: &Value) -> Result<Vec<f64>, String> {
    v.as_array().ok_or_else(|| format!("not a list: {v}"))?.iter().map(|x| x.as_f64().ok_or_else(|| format!("not a number: {x}"))).collect()
}

/// A 2D coefficient array `c[i][j]` (numpy `atleast_2d`: a scalar is 1x1, a flat list one row).
pub type Poly2 = Vec<Vec<f64>>;

fn poly2(v: &Value) -> Result<Poly2, String> {
    match v {
        Value::Number(_) => Ok(vec![vec![num(v)]]),
        Value::Array(a) if a.iter().all(Value::is_number) => Ok(vec![f64_list(v)?]),
        Value::Array(a) => {
            let rows: Vec<Vec<f64>> = a.iter().map(f64_list).collect::<Result<_, _>>()?;
            let w = rows.first().map_or(0, Vec::len);
            if rows.iter().any(|r| r.len() != w) {
                return Err("ragged coefficient array".into());
            }
            Ok(rows)
        }
        _ => Err(format!("not a coefficient array: {v}")),
    }
}

// ------------------------------------------------------------------------------------------
// truths

/// A rational polynomial patch `(hx, hy, hz) / hw` on a parameter box: the `param` truth.
#[derive(Clone, Debug)]
pub struct ParamPatch {
    hom: [Poly2; 4],
    dhu: [Poly2; 4],
    dhv: [Poly2; 4],
    domain: [f64; 4],
    /// Seeds: the 41 x 41 grid of the domain (u major), with the patch evaluated there.
    seeds: Vec<(f64, f64, V3)>,
}

/// One exact surface description of a case (`truth` entries).
#[derive(Clone, Debug)]
pub enum Truth {
    Plane { point: V3, normal: V3 },
    Sphere { center: V3, radius: f64 },
    Cylinder { point: V3, axis: V3, radius: f64 },
    Cone { point: V3, axis: V3, half_angle_deg: f64 },
    Torus { point: V3, axis: V3, major: f64, minor: f64 },
    HeightField { coeffs: Poly2, axes: [usize; 3] },
    Param(Box<ParamPatch>),
    Quadric { center: V3, axes: [V3; 3], abc: V3, signs: V3 },
    Ellipsoid { center: V3, axes: [V3; 3], abc: V3 },
}

fn axes3(v: &Value) -> Result<[V3; 3], String> {
    let a = v.as_array().ok_or("axes: not a list")?;
    if a.len() != 3 {
        return Err("axes: need 3 rows".into());
    }
    Ok([v3(&a[0])?, v3(&a[1])?, v3(&a[2])?])
}

impl Truth {
    pub fn from_json(t: &Value) -> Result<Truth, String> {
        let kind = field(t, "kind")?.as_str().ok_or("kind: not a string")?;
        Ok(match kind {
            "plane" => Truth::Plane { point: v3(field(t, "point")?)?, normal: v3(field(t, "normal")?)? },
            "sphere" => Truth::Sphere { center: v3(field(t, "center")?)?, radius: f64_field(t, "radius")? },
            "cylinder" => Truth::Cylinder { point: v3(field(t, "point")?)?, axis: v3(field(t, "axis")?)?, radius: f64_field(t, "radius")? },
            "cone" => Truth::Cone { point: v3(field(t, "point")?)?, axis: v3(field(t, "axis")?)?, half_angle_deg: f64_field(t, "half_angle_deg")? },
            "torus" => {
                Truth::Torus { point: v3(field(t, "point")?)?, axis: v3(field(t, "axis")?)?, major: f64_field(t, "R")?, minor: f64_field(t, "r")? }
            }
            "heightfield" => {
                let axes = match t.get("axes") {
                    None => [0, 1, 2],
                    Some(a) => {
                        let a = f64_list(a)?;
                        if a.len() != 3 || a.iter().any(|&x| !(0.0..=2.0).contains(&x)) {
                            return Err("heightfield axes".into());
                        }
                        [a[0] as usize, a[1] as usize, a[2] as usize]
                    }
                };
                Truth::HeightField { coeffs: poly2(field(t, "coeffs")?)?, axes }
            }
            "param" => {
                let hom = field(t, "hom")?.as_array().ok_or("hom: not a list")?;
                if hom.len() != 4 {
                    return Err("hom: need 4 coefficient arrays".into());
                }
                let hom = [poly2(&hom[0])?, poly2(&hom[1])?, poly2(&hom[2])?, poly2(&hom[3])?];
                let d = f64_list(field(t, "domain")?)?;
                if d.len() != 4 {
                    return Err("param domain".into());
                }
                Truth::Param(Box::new(ParamPatch::new(hom, [d[0], d[1], d[2], d[3]])))
            }
            "quadric" => {
                let abc = f64_list(field(t, "abc")?)?;
                let sg = f64_list(field(t, "signs")?)?;
                if abc.len() != 3 || sg.len() != 3 {
                    return Err("quadric abc/signs".into());
                }
                Truth::Quadric {
                    center: v3(field(t, "center")?)?,
                    axes: axes3(field(t, "axes")?)?,
                    abc: [abc[0], abc[1], abc[2]],
                    signs: [sg[0], sg[1], sg[2]],
                }
            }
            "ellipsoid" => {
                let abc = f64_list(field(t, "abc")?)?;
                if abc.len() != 3 {
                    return Err("ellipsoid abc".into());
                }
                Truth::Ellipsoid { center: v3(field(t, "center")?)?, axes: axes3(field(t, "axes")?)?, abc: [abc[0], abc[1], abc[2]] }
            }
            other => return Err(format!("unknown truth kind {other}")),
        })
    }

    /// Distance from each point to the surface (`truth_distance`): exact for the implicit kinds
    /// except the height field, quadric and ellipsoid (first order), closest point for `param`.
    pub fn distance(&self, pts: &[V3]) -> Vec<f64> {
        pts.iter().map(|&p| self.distance1(p)).collect()
    }

    pub fn distance1(&self, p: V3) -> f64 {
        match self {
            Truth::Plane { point, normal } => dot(sub(p, *point), unit(*normal)).abs(),
            Truth::Sphere { center, radius } => (norm(sub(p, *center)) - radius).abs(),
            Truth::Cylinder { point, axis, radius } => {
                let (_, rho) = axial(p, *point, *axis);
                (rho - radius).abs()
            }
            Truth::Cone { point, axis, half_angle_deg } => {
                let (h, rho) = axial(p, *point, *axis);
                let al = half_angle_deg.to_radians();
                (rho * al.cos() - h * al.sin()).abs()
            }
            Truth::Torus { point, axis, major, minor } => {
                let (h, rho) = axial(p, *point, *axis);
                ((rho - major).hypot(h) - minor).abs()
            }
            Truth::HeightField { coeffs, axes } => {
                let (x, y, z) = (p[axes[0]], p[axes[1]], p[axes[2]]);
                let (mut h, mut hx, mut hy) = (0.0, 0.0, 0.0);
                for (i, row) in coeffs.iter().enumerate() {
                    for (j, &c) in row.iter().enumerate() {
                        if c == 0.0 {
                            continue;
                        }
                        h += c * powi_np(x, i) * powi_np(y, j);
                        if i > 0 {
                            hx += c * i as f64 * powi_np(x, i - 1) * powi_np(y, j);
                        }
                        if j > 0 {
                            hy += c * j as f64 * powi_np(x, i) * powi_np(y, j - 1);
                        }
                    }
                }
                (z - h).abs() / (1.0 + hx * hx + hy * hy).sqrt()
            }
            Truth::Param(patch) => patch.distance(p),
            Truth::Quadric { center, axes, abc, signs } => quadric_distance(p, *center, axes, *abc, Some(*signs)),
            Truth::Ellipsoid { center, axes, abc } => quadric_distance(p, *center, axes, *abc, None),
        }
    }
}

/// (height along the unit axis, distance from the axis).
fn axial(p: V3, point: V3, axis: V3) -> (f64, f64) {
    let a = unit(axis);
    let q = sub(p, point);
    let h = dot(q, a);
    (h, norm([q[0] - h * a[0], q[1] - h * a[1], q[2] - h * a[2]]))
}

fn quadric_distance(p: V3, center: V3, axes: &[V3; 3], abc: V3, signs: Option<V3>) -> f64 {
    let q = sub(p, center);
    let l: V3 = [0, 1, 2].map(|k| dot(q, axes[k]) / abc[k]);
    let (e, w) = match signs {
        Some(sg) => {
            let e = (sg[0] * l[0] * l[0] + sg[1] * l[1] * l[1] + sg[2] * l[2] * l[2]) - 1.0;
            (e, [0, 1, 2].map(|k| 2.0 * (sg[k] * l[k] / abc[k])))
        }
        None => {
            let e = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]) - 1.0;
            (e, [0, 1, 2].map(|k| 2.0 * (l[k] / abc[k])))
        }
    };
    let grad: V3 = [0, 1, 2].map(|m| w[0] * axes[0][m] + w[1] * axes[1][m] + w[2] * axes[2][m]);
    e.abs() / norm(grad)
}

/// `c(u, w)` with the powers `up[i] = u**i`, `wp[j] = w**j` precomputed (the same `pow` calls
/// numpy makes, once per exponent instead of once per term).
fn p2_eval_pow(c: &Poly2, up: &[f64], wp: &[f64]) -> f64 {
    let mut out = 0.0;
    for (row, &ui) in c.iter().zip(up) {
        for (&cij, &wj) in row.iter().zip(wp) {
            if cij != 0.0 {
                out += cij * ui * wj;
            }
        }
    }
    out
}

fn pow_table(x: f64, n: usize) -> Vec<f64> {
    (0..n).map(|i| powi_np(x, i)).collect()
}

fn p2_du(c: &Poly2) -> Poly2 {
    let cols = c.first().map_or(1, Vec::len);
    if c.len() <= 1 {
        return vec![vec![0.0; cols]];
    }
    c.iter().enumerate().skip(1).map(|(i, row)| row.iter().map(|&x| x * i as f64).collect()).collect()
}

fn p2_dv(c: &Poly2) -> Poly2 {
    c.iter().map(|row| if row.len() <= 1 { vec![0.0] } else { row.iter().enumerate().skip(1).map(|(j, &x)| x * j as f64).collect() }).collect()
}

const N_SEED: usize = 41;

impl ParamPatch {
    fn new(hom: [Poly2; 4], domain: [f64; 4]) -> ParamPatch {
        let dhu = [p2_du(&hom[0]), p2_du(&hom[1]), p2_du(&hom[2]), p2_du(&hom[3])];
        let dhv = [p2_dv(&hom[0]), p2_dv(&hom[1]), p2_dv(&hom[2]), p2_dv(&hom[3])];
        let mut patch = ParamPatch { hom, dhu, dhv, domain, seeds: Vec::new() };
        let us = linspace(domain[0], domain[1], N_SEED);
        let vs = linspace(domain[2], domain[3], N_SEED);
        let mut seeds = Vec::with_capacity(N_SEED * N_SEED);
        for &u in &us {
            for &v in &vs {
                seeds.push((u, v, patch.eval(u, v)));
            }
        }
        patch.seeds = seeds;
        patch
    }

    fn pows(&self, u: f64, v: f64) -> (Vec<f64>, Vec<f64>) {
        let rows = self.hom.iter().map(Vec::len).max().unwrap_or(1);
        let cols = self.hom.iter().flat_map(|c| c.iter().map(Vec::len)).max().unwrap_or(1);
        (pow_table(u, rows), pow_table(v, cols))
    }

    pub fn eval(&self, u: f64, v: f64) -> V3 {
        let (up, vp) = self.pows(u, v);
        let h = [0, 1, 2, 3].map(|k| p2_eval_pow(&self.hom[k], &up, &vp));
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    }

    fn derivs(&self, u: f64, v: f64) -> (V3, V3, V3) {
        let (up, vp) = self.pows(u, v);
        let h = [0, 1, 2, 3].map(|k| p2_eval_pow(&self.hom[k], &up, &vp));
        let hu = [0, 1, 2, 3].map(|k| p2_eval_pow(&self.dhu[k], &up, &vp));
        let hv = [0, 1, 2, 3].map(|k| p2_eval_pow(&self.dhv[k], &up, &vp));
        let s: V3 = [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
        let su: V3 = [0, 1, 2].map(|k| (hu[k] - s[k] * hu[3]) / h[3]);
        let sv: V3 = [0, 1, 2].map(|k| (hv[k] - s[k] * hv[3]) / h[3]);
        (s, su, sv)
    }

    /// `param_distance`: Gauss-Newton closest point from the 12 nearest grid seeds in turn,
    /// clamped to the domain, until one lands within 1e-12.
    pub fn distance(&self, x: V3) -> f64 {
        let d2: Vec<f64> = self
            .seeds
            .iter()
            .map(|&(_, _, g)| {
                let d = sub(x, g);
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
            })
            .collect();
        let mut order: Vec<usize> = (0..d2.len()).collect();
        order.sort_by(|&a, &b| d2[a].total_cmp(&d2[b]));
        let d0 = self.domain;
        let mut out = f64::INFINITY;
        for &seed in order.iter().take(12) {
            if !(out > 1e-12) {
                break;
            }
            let (mut u, mut v, _) = self.seeds[seed];
            for _ in 0..40 {
                let (s, su, sv) = self.derivs(u, v);
                let r = sub(s, x);
                let a11 = dot(su, su);
                let a12 = dot(su, sv);
                let a22 = dot(sv, sv);
                let b1 = -dot(su, r);
                let b2 = -dot(sv, r);
                let mut det = a11 * a22 - a12 * a12;
                if det.abs() < 1e-300 {
                    det = 1e-300;
                }
                let du = (a22 * b1 - a12 * b2) / det;
                let dv = (a11 * b2 - a12 * b1) / det;
                u = clip(u + du, d0[0], d0[1]);
                v = clip(v + dv, d0[2], d0[3]);
            }
            out = np_minimum(out, norm(sub(self.eval(u, v), x)));
        }
        out
    }
}

// ------------------------------------------------------------------------------------------
// B-spline / NURBS surfaces of the case specs

/// `bspline_basis`: the knot span containing `t` and the `p + 1` nonzero basis functions there.
pub fn bspline_basis(knots: &[f64], p: usize, t: f64) -> (usize, Vec<f64>) {
    let n = knots.len().saturating_sub(p + 2);
    let span = if knots.get(n + 1).is_some_and(|&k| t >= k) {
        n
    } else {
        let right = knots.partition_point(|&k| k <= t);
        (right.saturating_sub(1)).clamp(p.min(n), n)
    };
    let mut nb = vec![0.0; p + 1];
    nb[0] = 1.0;
    let mut left = vec![0.0; p + 1];
    let mut rightv = vec![0.0; p + 1];
    for j in 1..=p {
        left[j] = t - knots.get((span + 1).wrapping_sub(j)).copied().unwrap_or(f64::NAN);
        rightv[j] = knots.get(span + j).copied().unwrap_or(f64::NAN) - t;
        let mut saved = 0.0;
        for r in 0..j {
            let tmp = nb[r] / (rightv[r + 1] + left[j - r]);
            nb[r] = saved + rightv[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        nb[j] = saved;
    }
    (span, nb)
}

/// A tensor-product NURBS surface from a case's `{"type": "nurbs", ...}` spec.
#[derive(Clone, Debug)]
pub struct Nurbs {
    pub p: usize,
    pub q: usize,
    pub knots_u: Vec<f64>,
    pub knots_v: Vec<f64>,
    /// Homogeneous control net (wx, wy, wz, w), `nu x nv`, row-major along u.
    net: Vec<[f64; 4]>,
    nv: usize,
}

impl Nurbs {
    pub fn from_spec(s: &Value) -> Result<Nurbs, String> {
        let deg = f64_list(field(s, "degree")?)?;
        if deg.len() != 2 || deg.iter().any(|&d| !(1.0..=30.0).contains(&d)) {
            return Err("nurbs degree".into());
        }
        let (p, q) = (deg[0] as usize, deg[1] as usize);
        let knots_u = f64_list(field(s, "knots_u")?)?;
        let knots_v = f64_list(field(s, "knots_v")?)?;
        let nu = f64_field(s, "nu")? as usize;
        let nv = f64_field(s, "nv")? as usize;
        let pts = v3_list(Some(field(s, "points")?))?;
        let w = f64_list(field(s, "weights")?)?;
        if nu <= p || nv <= q || pts.len() != nu * nv || w.len() != nu * nv || knots_u.len() != nu + p + 1 || knots_v.len() != nv + q + 1 {
            return Err("nurbs: inconsistent sizes".into());
        }
        let net = pts.iter().zip(&w).map(|(pt, &wt)| [pt[0] * wt, pt[1] * wt, pt[2] * wt, wt]).collect();
        Ok(Nurbs { p, q, knots_u, knots_v, net, nv })
    }

    /// The point at (u, v) of the natural parameter box.
    pub fn eval(&self, u: f64, v: f64) -> V3 {
        let (su, nu) = bspline_basis(&self.knots_u, self.p, u);
        let (sv, nvb) = bspline_basis(&self.knots_v, self.q, v);
        let mut h = [0.0; 4];
        for (i, bu) in nu.iter().enumerate() {
            for (j, bv) in nvb.iter().enumerate() {
                let Some(c) = self.net.get((su - self.p + i) * self.nv + (sv - self.q + j)) else { continue };
                for k in 0..4 {
                    h[k] += bu * bv * c[k];
                }
            }
        }
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    }

    /// The natural parameter box `[u0, u1, v0, v1]`.
    pub fn domain(&self) -> [f64; 4] {
        let f = |k: &[f64], first: bool| if first { k.first().copied() } else { k.last().copied() }.unwrap_or(f64::NAN);
        [f(&self.knots_u, true), f(&self.knots_u, false), f(&self.knots_v, true), f(&self.knots_v, false)]
    }
}
