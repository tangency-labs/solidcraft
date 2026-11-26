//! Exact distances from points to the true surfaces of an SSI corpus case.
//!
//! Ported from jyviko's ssi-corpus `scoring/ssi_geom.py` (`truth_distance`, `param_eval`,
//! `param_derivs`, `param_distance`, the `p2_*` polynomial helpers they need),
//! <https://github.com/jyviko/ssi-corpus> at commit `b04c8a6`, licensed MIT OR Apache-2.0.
//! The surface and truth generators of that file are not ported.
//!
//! Deviations: `param_distance` ranks the grid seeds with a stable sort (numpy's
//! `argsort` is not stable; only exact ties can differ).

use serde_json::Value;

use crate::HarnessError;

/// A 3D point.
pub type P3 = [f64; 3];

/// Power-basis coefficients `c[i][j]` of `u^i w^j` (rectangular).
pub type Poly2 = Vec<Vec<f64>>;

/// The implicit (or parametric) description of one true surface (`truth` of a case).
#[derive(Debug, Clone, PartialEq)]
pub enum Truth {
    /// `|(P - point) . n|`.
    Plane {
        /// A point of the plane.
        point: P3,
        /// Its normal (unitised on use).
        normal: P3,
    },
    /// `| |P - center| - radius |`.
    Sphere {
        /// Centre.
        center: P3,
        /// Radius.
        radius: f64,
    },
    /// `|rho - radius|`.
    Cylinder {
        /// A point of the axis.
        point: P3,
        /// Axis direction.
        axis: P3,
        /// Radius.
        radius: f64,
    },
    /// `|rho cos(a) - h sin(a)|` (the nappe `h >= 0`, valid near the surface).
    Cone {
        /// Apex.
        point: P3,
        /// Axis direction.
        axis: P3,
        /// Half angle (degrees).
        half_angle_deg: f64,
    },
    /// `|hypot(rho - R, h) - r|`.
    Torus {
        /// Centre.
        point: P3,
        /// Axis direction.
        axis: P3,
        /// Major radius.
        major: f64,
        /// Minor radius.
        minor: f64,
    },
    /// First-order distance to `z = h(x, y)` (coordinates permuted by `axes`).
    Heightfield {
        /// `h` as power coefficients `c[i][j] a^i b^j`.
        coeffs: Poly2,
        /// Indices of `(a, b, dependent)`.
        axes: [usize; 3],
    },
    /// Closest point on a rational polynomial patch.
    Param {
        /// Homogeneous coordinates `(Hx, Hy, Hz, Hw)` as power coefficients.
        hom: [Poly2; 4],
        /// `[u0, u1, v0, v1]`.
        domain: [f64; 4],
    },
    /// First-order distance to `sum_i signs[i] (L_i / abc_i)^2 = 1`.
    Quadric {
        /// Centre.
        center: P3,
        /// Local axes (rows).
        axes: [P3; 3],
        /// Semi-axes.
        abc: P3,
        /// Signs of the terms.
        signs: P3,
    },
    /// First-order distance to an ellipsoid (a quadric with all signs `+1`).
    Ellipsoid {
        /// Centre.
        center: P3,
        /// Local axes (rows).
        axes: [P3; 3],
        /// Semi-axes.
        abc: P3,
    },
}

fn err(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `v[key]` as a number.
pub fn f(v: &Value, key: &str) -> Result<f64, HarnessError> {
    v.get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| err(format!("missing number `{key}`")))
}

/// A JSON array of numbers.
pub fn nums(v: &Value) -> Result<Vec<f64>, HarnessError> {
    v.as_array()
        .ok_or_else(|| err("expected an array of numbers"))?
        .iter()
        .map(|x| x.as_f64().ok_or_else(|| err("expected a number")))
        .collect()
}

/// A JSON array of `N` numbers.
pub fn arr<const N: usize>(v: &Value) -> Result<[f64; N], HarnessError> {
    let n = nums(v)?;
    <[f64; N]>::try_from(n.as_slice()).map_err(|_| err(format!("expected {N} numbers")))
}

/// `v[key]` as `N` numbers.
pub fn arr_at<const N: usize>(v: &Value, key: &str) -> Result<[f64; N], HarnessError> {
    arr(v.get(key).ok_or_else(|| err(format!("missing `{key}`")))?)
}

/// A list of 3D points (`[[x, y, z], ...]`).
pub fn points(v: &Value) -> Result<Vec<P3>, HarnessError> {
    v.as_array()
        .ok_or_else(|| err("expected a point list"))?
        .iter()
        .map(arr::<3>)
        .collect()
}

/// numpy `atleast_2d(asarray(c))` of a scalar, 1D or 2D coefficient list.
fn poly2(v: &Value) -> Result<Poly2, HarnessError> {
    if let Some(x) = v.as_f64() {
        return Ok(vec![vec![x]]);
    }
    let a = v.as_array().ok_or_else(|| err("bad coefficients"))?;
    if a.iter().all(Value::is_number) {
        return Ok(vec![nums(v)?]);
    }
    let rows: Vec<Vec<f64>> = a.iter().map(nums).collect::<Result<_, _>>()?;
    let w = rows.first().map_or(0, Vec::len);
    if w == 0 || rows.iter().any(|r| r.len() != w) {
        return Err(err("ragged coefficient array"));
    }
    Ok(rows)
}

fn axes3(v: &Value, key: &str) -> Result<[P3; 3], HarnessError> {
    let a = v
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| err(format!("missing `{key}`")))?;
    match a.as_slice() {
        [x, y, z] => Ok([arr(x)?, arr(y)?, arr(z)?]),
        _ => Err(err(format!("`{key}` needs 3 rows"))),
    }
}

impl Truth {
    /// Parses one entry of a case's `truth` list.
    pub fn parse(v: &Value) -> Result<Self, HarnessError> {
        let kind = v
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| err("truth without kind"))?;
        Ok(match kind {
            "plane" => Self::Plane {
                point: arr_at(v, "point")?,
                normal: arr_at(v, "normal")?,
            },
            "sphere" => Self::Sphere {
                center: arr_at(v, "center")?,
                radius: f(v, "radius")?,
            },
            "cylinder" => Self::Cylinder {
                point: arr_at(v, "point")?,
                axis: arr_at(v, "axis")?,
                radius: f(v, "radius")?,
            },
            "cone" => Self::Cone {
                point: arr_at(v, "point")?,
                axis: arr_at(v, "axis")?,
                half_angle_deg: f(v, "half_angle_deg")?,
            },
            "torus" => Self::Torus {
                point: arr_at(v, "point")?,
                axis: arr_at(v, "axis")?,
                major: f(v, "R")?,
                minor: f(v, "r")?,
            },
            "heightfield" => {
                let axes = match v.get("axes") {
                    Some(a) => {
                        let n = nums(a)?;
                        match n.as_slice() {
                            [a0, a1, a2] => [*a0 as usize, *a1 as usize, *a2 as usize],
                            _ => return Err(err("heightfield axes need 3 entries")),
                        }
                    }
                    None => [0, 1, 2],
                };
                if axes.iter().any(|&a| a > 2) {
                    return Err(err("heightfield axis out of range"));
                }
                Self::Heightfield {
                    coeffs: poly2(v.get("coeffs").ok_or_else(|| err("missing coeffs"))?)?,
                    axes,
                }
            }
            "param" => {
                let h = v
                    .get("hom")
                    .and_then(Value::as_array)
                    .ok_or_else(|| err("missing hom"))?;
                let hom = match h.as_slice() {
                    [a, b, c, d] => [poly2(a)?, poly2(b)?, poly2(c)?, poly2(d)?],
                    _ => return Err(err("hom needs 4 polynomials")),
                };
                Self::Param {
                    hom,
                    domain: arr_at(v, "domain")?,
                }
            }
            "quadric" => Self::Quadric {
                center: arr_at(v, "center")?,
                axes: axes3(v, "axes")?,
                abc: arr_at(v, "abc")?,
                signs: arr_at(v, "signs")?,
            },
            "ellipsoid" => Self::Ellipsoid {
                center: arr_at(v, "center")?,
                axes: axes3(v, "axes")?,
                abc: arr_at(v, "abc")?,
            },
            other => return Err(err(format!("unknown truth kind `{other}`"))),
        })
    }
}

/// `a . b`.
pub fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `a - b`.
pub fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `|a|` (numpy `linalg.norm`).
pub fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}

/// `|a - b|`.
pub fn dist(a: P3, b: P3) -> f64 {
    norm(sub(a, b))
}

/// `v / |v|`.
fn unit(v: P3) -> P3 {
    let n = norm(v);
    [v[0] / n, v[1] / n, v[2] / n]
}

/// numpy `x ** i` for a small integer power.
fn powi(x: f64, i: usize) -> f64 {
    match i {
        0 => 1.0,
        1 => x,
        2 => x * x,
        _ => x.powf(i as f64),
    }
}

/// `p2_eval`: `sum c[i][j] u^i w^j` over the nonzero coefficients.
fn p2_eval(c: &Poly2, u: f64, w: f64) -> f64 {
    let mut out = 0.0;
    for (i, row) in c.iter().enumerate() {
        for (j, &cij) in row.iter().enumerate() {
            if cij != 0.0 {
                out += cij * powi(u, i) * powi(w, j);
            }
        }
    }
    out
}

/// `_p2_du`.
fn p2_du(c: &Poly2) -> Poly2 {
    if c.len() <= 1 {
        let w = c.first().map_or(1, Vec::len);
        return vec![vec![0.0; w]];
    }
    c.iter()
        .enumerate()
        .skip(1)
        .map(|(i, row)| row.iter().map(|x| x * i as f64).collect())
        .collect()
}

/// `_p2_dv`.
fn p2_dv(c: &Poly2) -> Poly2 {
    c.iter()
        .map(|row| {
            if row.len() <= 1 {
                vec![0.0]
            } else {
                row.iter()
                    .enumerate()
                    .skip(1)
                    .map(|(j, x)| x * j as f64)
                    .collect()
            }
        })
        .collect()
}

/// Precomputed patch for [`param_distance`].
struct Patch {
    h: [Poly2; 4],
    hu: [Poly2; 4],
    hv: [Poly2; 4],
}

impl Patch {
    fn new(hom: &[Poly2; 4]) -> Self {
        Self {
            h: hom.clone(),
            hu: [
                p2_du(&hom[0]),
                p2_du(&hom[1]),
                p2_du(&hom[2]),
                p2_du(&hom[3]),
            ],
            hv: [
                p2_dv(&hom[0]),
                p2_dv(&hom[1]),
                p2_dv(&hom[2]),
                p2_dv(&hom[3]),
            ],
        }
    }

    /// `param_eval`.
    fn eval(&self, u: f64, v: f64) -> P3 {
        let h = [
            p2_eval(&self.h[0], u, v),
            p2_eval(&self.h[1], u, v),
            p2_eval(&self.h[2], u, v),
            p2_eval(&self.h[3], u, v),
        ];
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    }

    /// `param_derivs`: `(S, Su, Sv)`.
    fn derivs(&self, u: f64, v: f64) -> (P3, P3, P3) {
        let e = |c: &[Poly2; 4]| {
            [
                p2_eval(&c[0], u, v),
                p2_eval(&c[1], u, v),
                p2_eval(&c[2], u, v),
                p2_eval(&c[3], u, v),
            ]
        };
        let h = e(&self.h);
        let hu = e(&self.hu);
        let hv = e(&self.hv);
        let s = [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
        let su = [
            (hu[0] - s[0] * hu[3]) / h[3],
            (hu[1] - s[1] * hu[3]) / h[3],
            (hu[2] - s[2] * hu[3]) / h[3],
        ];
        let sv = [
            (hv[0] - s[0] * hv[3]) / h[3],
            (hv[1] - s[1] * hv[3]) / h[3],
            (hv[2] - s[2] * hv[3]) / h[3],
        ];
        (s, su, sv)
    }
}

/// numpy `linspace(a, b, n)`.
pub fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![a];
    }
    let div = (n - 1) as f64;
    let step = (b - a) / div;
    let mut v: Vec<f64> = (0..n).map(|i| i as f64 * step + a).collect();
    if let Some(last) = v.last_mut() {
        *last = b;
    }
    v
}

/// `param_distance` (`n_seed = 41`): Gauss-Newton closest point from the nearest grid
/// seeds (clamped to the domain), retried from up to 12 seeds while the distance is not
/// tiny (the patch may overlap itself).
fn param_distance(hom: &[Poly2; 4], d0: [f64; 4], pts: &[P3]) -> Vec<f64> {
    const N_SEED: usize = 41;
    const RANKS: usize = 12;
    let patch = Patch::new(hom);
    let us = linspace(d0[0], d0[1], N_SEED);
    let vs = linspace(d0[2], d0[3], N_SEED);
    // meshgrid(indexing="ij").ravel(): u outer, v inner
    let mut seeds: Vec<(f64, f64, P3)> = Vec::with_capacity(N_SEED * N_SEED);
    for &u in &us {
        for &v in &vs {
            seeds.push((u, v, patch.eval(u, v)));
        }
    }
    let mut out = Vec::with_capacity(pts.len());
    for &x in pts {
        let mut order: Vec<(f64, usize)> = seeds
            .iter()
            .enumerate()
            .map(|(k, s)| {
                let d = sub(x, s.2);
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2], k)
            })
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut best = f64::INFINITY;
        for &(_, k) in order.iter().take(RANKS) {
            // `todo = out > 1e-12`
            if best.partial_cmp(&1e-12) != Some(core::cmp::Ordering::Greater) {
                break;
            }
            let Some(&(mut u, mut v, _)) = seeds.get(k) else {
                continue;
            };
            for _ in 0..40 {
                let (s, su, sv) = patch.derivs(u, v);
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
                u = np_clip(u + du, d0[0], d0[1]);
                v = np_clip(v + dv, d0[2], d0[3]);
            }
            let d = dist(patch.eval(u, v), x);
            best = np_fmin(best, d);
        }
        out.push(best);
    }
    out
}

/// numpy `clip` (NaN propagates).
fn np_clip(x: f64, lo: f64, hi: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

/// numpy `minimum` (NaN propagates).
fn np_fmin(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if b < a {
        b
    } else {
        a
    }
}

/// `truth_distance(truth, P)`: the distance of each point to the true surface.
pub fn truth_distance(t: &Truth, pts: &[P3]) -> Vec<f64> {
    match t {
        Truth::Plane { point, normal } => {
            let n = unit(*normal);
            pts.iter().map(|&p| dot(sub(p, *point), n).abs()).collect()
        }
        Truth::Sphere { center, radius } => pts
            .iter()
            .map(|&p| (dist(p, *center) - radius).abs())
            .collect(),
        Truth::Cylinder { point, axis, .. }
        | Truth::Cone { point, axis, .. }
        | Truth::Torus { point, axis, .. } => {
            let a = unit(*axis);
            pts.iter()
                .map(|&p| {
                    let q = sub(p, *point);
                    let h = dot(q, a);
                    let rho = norm([q[0] - h * a[0], q[1] - h * a[1], q[2] - h * a[2]]);
                    match t {
                        Truth::Cylinder { radius, .. } => (rho - radius).abs(),
                        Truth::Cone { half_angle_deg, .. } => {
                            let al = half_angle_deg.to_radians();
                            (rho * al.cos() - h * al.sin()).abs()
                        }
                        Truth::Torus { major, minor, .. } => ((rho - major).hypot(h) - minor).abs(),
                        _ => f64::NAN,
                    }
                })
                .collect()
        }
        Truth::Heightfield { coeffs, axes } => pts
            .iter()
            .map(|p| {
                let pick = |k: usize| p.get(k).copied().unwrap_or(f64::NAN);
                let (x, y, z) = (pick(axes[0]), pick(axes[1]), pick(axes[2]));
                let (mut h, mut hx, mut hy) = (0.0, 0.0, 0.0);
                for (i, row) in coeffs.iter().enumerate() {
                    for (j, &c) in row.iter().enumerate() {
                        if c == 0.0 {
                            continue;
                        }
                        h += c * powi(x, i) * powi(y, j);
                        if i > 0 {
                            hx += c * i as f64 * powi(x, i - 1) * powi(y, j);
                        }
                        if j > 0 {
                            hy += c * j as f64 * powi(x, i) * powi(y, j - 1);
                        }
                    }
                }
                (z - h).abs() / (1.0 + hx * hx + hy * hy).sqrt()
            })
            .collect(),
        Truth::Param { hom, domain } => param_distance(hom, *domain, pts),
        Truth::Quadric {
            center,
            axes,
            abc,
            signs,
        } => quadric(*center, axes, *abc, *signs, pts),
        Truth::Ellipsoid { center, axes, abc } => quadric(*center, axes, *abc, [1.0; 3], pts),
    }
}

/// First-order distance `|E| / |grad E|` to `sum sg_i (L_i / abc_i)^2 = 1`.
fn quadric(center: P3, axes: &[P3; 3], abc: P3, sg: P3, pts: &[P3]) -> Vec<f64> {
    pts.iter()
        .map(|&p| {
            let q = sub(p, center);
            let l = [
                dot(q, axes[0]) / abc[0],
                dot(q, axes[1]) / abc[1],
                dot(q, axes[2]) / abc[2],
            ];
            let e = sg[0] * l[0] * l[0] + sg[1] * l[1] * l[1] + sg[2] * l[2] * l[2] - 1.0;
            let g = [
                2.0 * (sg[0] * l[0] / abc[0]),
                2.0 * (sg[1] * l[1] / abc[1]),
                2.0 * (sg[2] * l[2] / abc[2]),
            ];
            let grad = [
                g[0] * axes[0][0] + g[1] * axes[1][0] + g[2] * axes[2][0],
                g[0] * axes[0][1] + g[1] * axes[1][1] + g[2] * axes[2][1],
                g[0] * axes[0][2] + g[1] * axes[1][2] + g[2] * axes[2][2],
            ];
            e.abs() / norm(grad)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytic_distances() {
        let s = Truth::Sphere {
            center: [0.0; 3],
            radius: 2.0,
        };
        assert_eq!(truth_distance(&s, &[[3.0, 0.0, 0.0]]), vec![1.0]);
        let t = Truth::Torus {
            point: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            major: 3.0,
            minor: 1.0,
        };
        let d = truth_distance(&t, &[[4.0, 0.0, 0.0], [3.0, 0.0, 2.0]]);
        assert!(d[0].abs() < 1e-15 && (d[1] - 1.0).abs() < 1e-15);
        let e = Truth::Ellipsoid {
            center: [0.0; 3],
            axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            abc: [1.0, 5.0, 2.0],
        };
        assert!(truth_distance(&e, &[[0.0, 5.0, 0.0]])[0].abs() < 1e-15);
    }

    #[test]
    fn param_patch_closest_point() {
        // the plane patch z = 0 over [0,1]^2: (u, v, 0, 1)
        let hom = [
            vec![vec![0.0], vec![1.0]],
            vec![vec![0.0, 1.0]],
            vec![vec![0.0]],
            vec![vec![1.0]],
        ];
        let t = Truth::Param {
            hom,
            domain: [0.0, 1.0, 0.0, 1.0],
        };
        let d = truth_distance(&t, &[[0.3, 0.4, 0.5], [2.0, 0.5, 0.0]]);
        assert!((d[0] - 0.5).abs() < 1e-12, "{d:?}");
        assert!((d[1] - 1.0).abs() < 1e-12, "{d:?}");
    }

    #[test]
    fn linspace_matches_numpy() {
        assert_eq!(linspace(0.0, 1.0, 3), vec![0.0, 0.5, 1.0]);
        assert_eq!(linspace(0.0, 1.0, 1), vec![0.0]);
    }
}
