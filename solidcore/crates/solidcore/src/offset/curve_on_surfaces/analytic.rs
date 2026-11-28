//! Closed-form distance bounds of a curve from a plane or a cone (a cylinder is a cone of
//! semi-angle 0): the analytic half of [`super::curve_on_surfaces`].
//!
//! Upstream: none (not upstream; solidcore-native).
//!
//! | Rust          | Upstream |
//! |---------------|----------|
//! | [`Analytic`]  | none     |
//! | [`Path`]      | none     |
//!
//! **The bound.** For a surface `S` take a function `F` with `|F(x)|` the distance from `x`
//! to a point of `S`'s unbounded form (the foot):
//!
//! - a plane through `O` spanned by `X` and `Y`, unit normal `n = X x Y / |X x Y|`:
//!   `F(x) = (x - O) . n`, the foot `x - F(x) n`, whose plane coordinates are `x`'s (the
//!   Gram system of `X` and `Y` solved by Cramer's rule, so `X` and `Y` need not be
//!   orthonormal);
//! - a cone `O + R(v) (cos u X + sin u Y) + v Z`, `R(v) = R0 + T v`: with `v(x)` the
//!   height of `x` along `Z` and `rho(x)` its distance from the axis, the surface point at
//!   height `v(x)` in `x`'s half-plane is `|rho(x) - R(v(x))|` from `x`, so
//!   `F(x) = rho(x) - R(v(x))` (when `R(v) < 0` that point is on the other nappe and the
//!   distance is `|rho + R|`, at most `|F|`).
//!
//! The curve `C` (as the kernel evaluates it: a B-spline or an ellipse's rational NURB, or
//! a line's segment) is cut into segments `[a, b]` inside one polynomial piece. On a
//! segment, `phi = F o C` misses its linear interpolation by at most
//! `h^2 / 8 sup |phi''|` (`h = b - a`), and `phi'' = grad F . C'' + C'^T H_F C'` is
//! enclosed over the segment's box: for a plane `n . C''`; for a cone, with `w` the part
//! of `x - O` across the axis, `rho = |w|` and `w^ = w / rho`, `grad F = w^ - T Z / |Z|^2`
//! and `H_F = (I - Z Z^T / |Z|^2 - w^ w^T) / rho` (the Hessian of `rho`; `v` and `R` are
//! linear), so `phi'' = w^ . C'' - T (Z . C'') / |Z|^2 + (|C'|^2 - (C' . Z)^2 / |Z|^2 -
//! (w^ . C')^2) / rho`, with `rho` proved positive. The distance is then at most
//! `max(|phi(a)|, |phi(b)|) + h^2 / 8 sup |phi''| + slack` on the segment. Every quantity
//! is enclosed in rounded interval arithmetic; a segment over the distance is cut in two,
//! to [`super::MAX_DEPTH`] halvings, within the edge's segment budget.
//!
//! **Domains** (the slack). A surface that does not evaluate out of bounds clamps its
//! parameters to its domain, so a foot past the domain is not a surface point: the point
//! at the clamped parameters is, at most `du su |X| + dv sv |Y|` from it on a plane (`du`,
//! `dv` how far the box's coordinates reach past the domain, `su`, `sv` the scales) and
//! `dv (|Z| + |T|)` on a cone (its height past the domain; the angular domain must be a
//! whole turn, else nothing is proved).
//!
//! **Cone frames** (the slack). The cone is taken about its stored `Z` (its height
//! `v = (x - O) . Z / |Z|^2`), its circles of radius `R(v)` across it. The placement's
//! `X`, `Y`, `Z` are enclosed off orthonormal by a residual `r` (the magnitudes of their
//! dot products and squared lengths less 1), refused past [`FRAME_ULPS`] ulps. The
//! allowance added is `8 r (|R| + |v| + |x - O|)`, enclosed with outward rounding; it holds
//! for every residual `r <= 1e-3` (far above the ulps allowed), with no term dropped:
//!
//! - `F` measures `x` against the ideal point `Q = O + v Z + R(v) w` at the same height
//!   (`v Z` is `x`'s exact projection on the axis), `w` the unit direction of `x`'s part
//!   across `Z`: `|x - Q| = |F(x)|`.
//! - The surface at the same `v` is `P(u) = O + v Z + R(v) c(u)`, `c(u) = cos u X +
//!   sin u Y`, so `|P(u) - Q| = |R(v)| |c(u) - w|`. With `Zh = Z / |Z|` (`|Z|^2 >= 1 -
//!   r`): `|c . Zh| <= (|X . Z| + |Y . Z|) / |Z| <= 2 r / sqrt(1 - r)`, and `|c|^2 = cos^2
//!   |X|^2 + sin^2 |Y|^2 + 2 cos sin X . Y` lies in `[1 - 2 r, 1 + 2 r]`.
//! - `c`'s part across `Z`, `c_p(u)`, traces an ellipse round the axis (`X`, `Y` nearly
//!   orthonormal, so independent across `Z`), which takes every direction: pick `u` with
//!   `c_p(u) = lambda w`, `lambda > 0`. Then `|lambda^2 - 1| <= 2 r + 4 r^2 / (1 - r)` and
//!   `|lambda - 1| <= |lambda^2 - 1|`, so `|c(u) - w| <= |c . Zh| + |lambda - 1| <=
//!   (2 / sqrt(1 - r) + 2 + 4 r / (1 - r)) r < 5 r` for `r <= 1e-3`.
//! - So `x` is within `|F(x)| + 5 r |R(v)|` of the surface, and `8 r (|R| + |v| + |x -
//!   O|)` (with `|R|` enclosed over the box) is at least `5 r |R(v)|`.
//!
//! The surface is taken as its formula with the stored axes; the rounding of its own
//! evaluation is not bounded (as for planes). The semi-angle's tangent is taken within
//! two ulps of the libm value.
//!
//! Deviations: none (no upstream counterpart). Not checked (nothing proved): other analytic
//! kinds (spheres and tori evaluate through their NURB), offsets of analytic surfaces, a
//! cone of partial turn, a segment touching a cone's axis. Recursion: none (an explicit
//! stack, at most [`super::MAX_DEPTH`] halvings of each piece).

use super::{add, charge, cross, norm_up, scale, sub, FIRST_SEGMENTS, IV3, MAX_DEPTH, OP};
use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::error::Result;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::Surface;

/// A curve as the kernel evaluates it, for the analytic check.
pub(super) enum Path<'c> {
    /// A NURBS curve: a B-spline, or the rational NURB an ellipse evaluates through.
    Nurbs(&'c NurbsCurve),
    /// A line over the interval: the segment `P0 + s (P1 - P0)`, `s` in `[0, 1]`, between
    /// two boxes holding its ends (`Line::segment_over`).
    Segment([IV3; 2]),
}

/// The most a cone's frame may be off orthonormal, in units of `f64::EPSILON`: each
/// enclosed `X . Y`, `X . Z`, `Y . Z`, `|X|^2 - 1`, `|Y|^2 - 1`, `|Z|^2 - 1`.
const FRAME_ULPS: f64 = 4.0;

/// A plane or cone the check bounds in closed form (module docs).
pub(super) enum Analytic {
    /// `F(x) = (x - O) . n`.
    Plane {
        /// The origin `O`.
        o: IV3,
        /// The unit normal, enclosed.
        n: IV3,
        /// The X and Y axes, their scales and the STEP domain the plane clamps its
        /// parameters to; `None` when the plane evaluates out of bounds.
        bounds: Option<(IV3, IV3, Ivl, Ivl, Extent2d)>,
    },
    /// `F(x) = rho(x) - (R0 + T v(x))`.
    Cone {
        /// The origin `O` on the axis.
        o: IV3,
        /// The axis `Z` as stored.
        z: IV3,
        /// `|Z|^2`, enclosed.
        zz: Ivl,
        /// `R0`.
        r0: Ivl,
        /// `T`, the semi-angle's tangent, enclosed.
        t: Ivl,
        /// The STEP heights the cone clamps its heights to; `None` when it evaluates out
        /// of bounds.
        heights: Option<Extent1d>,
        /// The frame's largest residual off orthonormal (at most [`FRAME_ULPS`] ulps).
        frame: f64,
    },
}

/// The largest residual of `axes` off orthonormal, enclosed: the magnitudes of their
/// pairwise dot products and of their squared lengths less 1.
fn frame_residual(axes: &[IV3]) -> f64 {
    let mut r: f64 = 0.0;
    for (i, a) in axes.iter().enumerate() {
        r = r.max((dot(*a, *a) - Ivl::from(1.0)).mag());
        for b in axes.iter().skip(i + 1) {
            r = r.max(dot(*a, *b).mag());
        }
    }
    r
}

/// How far `i` reaches past `e` (0 inside), rounded up; `None` when not finite.
fn past(i: Ivl, e: Extent1d) -> Option<f64> {
    if !finite(i) {
        return None;
    }
    let lo = round_up(e.min() - i.lo).max(0.0);
    let hi = round_up(i.hi - e.max()).max(0.0);
    let r = lo.max(hi);
    r.is_finite().then_some(r)
}

fn v3(v: Vec3) -> IV3 {
    V3 {
        x: Ivl::from(v.x),
        y: Ivl::from(v.y),
        z: Ivl::from(v.z),
    }
}

fn dot(a: IV3, b: IV3) -> Ivl {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn finite(i: Ivl) -> bool {
    i.lo.is_finite() && i.hi.is_finite()
}

impl Analytic {
    /// The plane or cone `s` is, where the check covers it.
    pub(super) fn of(s: &Surface) -> Option<Self> {
        match s.kind() {
            SurfaceKind::Plane(p) => {
                let pos = p.position();
                let (x, y) = (v3(pos.x_axis()), v3(pos.y_axis()));
                let n = cross(x, y);
                let len = dot(n, n).sqrt();
                if !(len.lo > 0.0 && finite(len)) {
                    return None;
                }
                let n = V3 {
                    x: n.x / len,
                    y: n.y / len,
                    z: n.z / len,
                };
                let sc = p.uv_scale();
                let bounds = (!p.base().is_out_of_bounds_enabled())
                    .then(|| (x, y, Ivl::from(sc.x), Ivl::from(sc.y), p.step_uv_domain()));
                Some(Self::Plane {
                    o: v3(pos.origin()),
                    n,
                    bounds,
                })
            }
            SurfaceKind::Cone(c) => {
                let rev = c.rev();
                let dom = rev.anal_uv_domain;
                let turn = dom.u_max() - dom.u_min();
                if turn.is_nan() || turn < 360.0 {
                    return None;
                }
                let pos = rev.position();
                let z = v3(pos.z_axis());
                let zz = dot(z, z);
                let t = c.tan_semi_angle();
                if !(zz.lo > 0.0 && finite(zz) && t.is_finite() && c.base_radius().is_finite()) {
                    return None;
                }
                let frame = frame_residual(&[v3(pos.x_axis()), v3(pos.y_axis()), z]);
                // (the allowance's derivation needs `r <= 1e-3`; this is far below it)
                if frame.is_nan() || frame > FRAME_ULPS * f64::EPSILON {
                    return None;
                }
                // within two ulps of the libm tangent
                let ulp = f64::EPSILON * t.abs();
                let heights = (!rev.is_out_of_bounds_enabled()).then(|| dom.v_interval());
                Some(Self::Cone {
                    o: v3(pos.origin()),
                    z,
                    zz,
                    r0: Ivl::from(c.base_radius()),
                    t: Ivl::checked(t - 2.0 * ulp, t + 2.0 * ulp),
                    heights,
                    frame,
                })
            }
            _ => None,
        }
    }

    /// `F` over the box `x`, `None` where it is not enclosed.
    fn f(&self, x: IV3) -> Option<Ivl> {
        let v = match self {
            Self::Plane { o, n, .. } => dot(sub(x, *o), *n),
            Self::Cone {
                o, z, zz, r0, t, ..
            } => {
                let q = sub(x, *o);
                let v = dot(q, *z) / *zz;
                let w = sub(q, scale(*z, v));
                let rho = dot(w, w).sqrt();
                rho - (*r0 + *t * v)
            }
        };
        finite(v).then_some(v)
    }

    /// For the points of the box `x`, a bound of the distance from the foot `F` gives to the
    /// surface point the kernel evaluates there (module docs: the parameters clamped to the
    /// domain; for a cone, also its frame's residual), `None` where it is not enclosed.
    fn slack(&self, x: IV3) -> Option<f64> {
        let e = match self {
            Self::Plane { bounds: None, .. } => 0.0,
            Self::Plane {
                o,
                bounds: Some((ax, ay, su, sv, dom)),
                ..
            } => {
                // the foot's plane coordinates, the Gram system solved by Cramer's rule
                let q = sub(x, *o);
                let (xx, xy, yy) = (dot(*ax, *ax), dot(*ax, *ay), dot(*ay, *ay));
                let (qx, qy) = (dot(q, *ax), dot(q, *ay));
                let det = xx * yy - xy * xy;
                if det.lo.is_nan() || det.lo <= 0.0 {
                    return None;
                }
                let a = (qx * yy - qy * xy) / det;
                let b = (qy * xx - qx * xy) / det;
                let du = past(a / *su, dom.u_interval())?;
                let dv = past(b / *sv, dom.v_interval())?;
                // the clamped point moves `du su X + dv sv Y` at most
                let mx = round_up(su.mag() * norm_up(*ax));
                let my = round_up(sv.mag() * norm_up(*ay));
                round_up(round_up(du * mx) + round_up(dv * my))
            }
            Self::Cone {
                o,
                z,
                zz,
                r0,
                t,
                heights,
                frame,
            } => {
                let q = sub(x, *o);
                let v = dot(q, *z) / *zz;
                let dv = match heights {
                    Some(h) => past(v, *h)?,
                    None => 0.0,
                };
                // a clamped height moves the point `dv (|Z| + |T|)` at most; the frame's
                // residual `r` puts the surface within `5 r |R|` of the ideal cone's
                // point, bounded above by `8 r (|R| + |v| + |x - O|)` (module docs)
                let zt = round_up(zz.sqrt().hi + t.mag());
                let r = (*r0 + *t * v).mag();
                let size = round_up(round_up(r + v.mag()) + norm_up(q));
                round_up(round_up(dv * zt) + round_up(8.0 * frame * size))
            }
        };
        e.is_finite().then_some(e)
    }

    /// `sup |phi''|` over the segment whose points lie in `x`, with derivatives in `d1`,
    /// `d2` (module docs), `None` where it is not enclosed or the box meets a cone's axis.
    fn second(&self, x: IV3, d1: IV3, d2: IV3) -> Option<f64> {
        let e = match self {
            Self::Plane { n, .. } => dot(*n, d2),
            Self::Cone { o, z, zz, t, .. } => {
                let q = sub(x, *o);
                let v = dot(q, *z) / *zz;
                let w = sub(q, scale(*z, v));
                let rho = dot(w, w).sqrt();
                if rho.lo.is_nan() || rho.lo <= 0.0 {
                    return None;
                }
                let hat = V3 {
                    x: w.x / rho,
                    y: w.y / rho,
                    z: w.z / rho,
                };
                let (cz, a) = (dot(d1, *z), dot(hat, d1));
                let grad = dot(hat, d2) - *t * dot(*z, d2) / *zz;
                let hess = (dot(d1, d1) - cz * cz / *zz - a * a) / rho;
                grad + hess
            }
        };
        finite(e).then(|| e.mag())
    }
}

/// A segment `[a, b]` of the curve enclosed: its end points, its points, and its first
/// and second derivatives over it.
struct Piece {
    pa: IV3,
    pb: IV3,
    x: IV3,
    d1: IV3,
    d2: IV3,
}

fn enclose(path: &Path<'_>, a: f64, b: f64) -> Option<Piece> {
    match path {
        Path::Nurbs(cn) => {
            let at = |t: f64| {
                curve_derivs_over(cn, Ivl::from(t), 0, None)
                    .ok()
                    .and_then(|d| d.first().copied())
            };
            let d = curve_derivs_over(cn, Ivl::new(a, b), 2, None).ok()?;
            Some(Piece {
                pa: at(a)?,
                pb: at(b)?,
                x: *d.first()?,
                d1: *d.get(1)?,
                d2: *d.get(2)?,
            })
        }
        Path::Segment([p0, p1]) => {
            let d = sub(*p1, *p0);
            let at = |s: Ivl| add(*p0, scale(d, s));
            Some(Piece {
                pa: at(Ivl::from(a)),
                pb: at(Ivl::from(b)),
                x: at(Ivl::new(a, b)),
                d1: d,
                d2: v3(Vec3::ZERO),
            })
        }
    }
}

/// Whether `path` over `interval` (the curve's parameters; `[0, 1]` for a segment) is
/// proved within `tol` of the surface (module docs), within the segment `budget`. `false`
/// where it is not proved.
///
/// # Errors
///
/// [`SolidError::Cancelled`](crate::error::SolidError::Cancelled) when the kernel's cancel
/// token fires (polled once per segment).
pub(super) fn on_analytic(
    k: &Kernel,
    path: &Path<'_>,
    s: &Analytic,
    interval: Extent1d,
    tol: f64,
    budget: &mut usize,
) -> Result<bool> {
    let (lo, hi) = (interval.min(), interval.max());
    let mut cuts = vec![lo];
    if let Path::Nurbs(cn) = path {
        for &u in &cn.u {
            if u > lo && u < hi && cuts.last().is_some_and(|&l| u > l) {
                cuts.push(u);
            }
        }
    }
    cuts.push(hi);
    for span in cuts.windows(2) {
        let [a0, b0] = span else { continue };
        let mut stack: Vec<(f64, f64, u32)> = Vec::new();
        for i in (0..FIRST_SEGMENTS).rev() {
            let f = |j: usize| {
                if j == FIRST_SEGMENTS {
                    *b0
                } else {
                    a0 + (b0 - a0) * (j as f64) / (FIRST_SEGMENTS as f64)
                }
            };
            stack.push((f(i), f(i + 1), 0));
        }
        while let Some((a, b, depth)) = stack.pop() {
            k.check_cancelled(OP)?;
            if !charge(budget) {
                return Ok(false);
            }
            let Some(p) = enclose(path, a, b) else {
                return Ok(false);
            };
            let (Some(fa), Some(fb), Some(extra)) = (s.f(p.pa), s.f(p.pb), s.slack(p.x)) else {
                return Ok(false);
            };
            let g = fa.mag().max(fb.mag());
            if g.is_nan() || g > tol {
                return Ok(false);
            }
            let h = round_up(b - a);
            let ok = s.second(p.x, p.d1, p.d2).is_some_and(|e| {
                let bend = round_up(round_up(round_up(h * h) / 8.0) * e);
                round_up(round_up(g + bend) + extra) <= tol
            });
            if ok {
                continue;
            }
            let m = 0.5 * (a + b);
            if depth >= MAX_DEPTH || !(a < m && m < b) {
                return Ok(false);
            }
            stack.push((m, b, depth + 1));
            stack.push((a, m, depth + 1));
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::det;

    /// The cone-frame allowance's lemma (module docs): for axes off orthonormal by a
    /// residual `r`, every unit direction `w` across `Z` is within `5 r` of some
    /// `c(u) = cos u X + sin u Y`, and the allowance `8 r (|R| + ...)` covers `5 r |R|`.
    /// Checked on skewed frames with `r` far above the ulps allowed (so the effect is not
    /// lost in rounding); the sampled minimum over `u` bounds the true one from above.
    #[test]
    fn a_skewed_frame_reaches_every_direction_within_five_residuals() {
        for (i, &e) in [1e-6, 1e-5, 1e-4].iter().enumerate() {
            let s = f64::from(u8::try_from(i).expect("index")) + 1.0;
            let x = Vec3::new(1.0 + 0.3 * e, 0.7 * e * s, -0.4 * e);
            let y = Vec3::new(-0.5 * e, 1.0 - 0.2 * e, 0.6 * e / s);
            let z = Vec3::new(0.3 * e, -0.8 * e, 1.0 + 0.1 * e);
            let r = frame_residual(&[v3(x), v3(y), v3(z)]);
            assert!(r > 0.0 && r <= 1e-3, "{r}");
            let zl = (z.x * z.x + z.y * z.y + z.z * z.z).sqrt();
            let zh = Vec3::new(z.x / zl, z.y / zl, z.z / zl);
            // an orthonormal pair across Z
            let a = {
                let d = x.x * zh.x + x.y * zh.y + x.z * zh.z;
                let p = Vec3::new(x.x - d * zh.x, x.y - d * zh.y, x.z - d * zh.z);
                let l = (p.x * p.x + p.y * p.y + p.z * p.z).sqrt();
                Vec3::new(p.x / l, p.y / l, p.z / l)
            };
            let b = Vec3::new(
                zh.y * a.z - zh.z * a.y,
                zh.z * a.x - zh.x * a.z,
                zh.x * a.y - zh.y * a.x,
            );
            for j in 0..64 {
                let th = f64::from(j) * core::f64::consts::TAU / 64.0;
                let w = Vec3::new(
                    det::cos(th) * a.x + det::sin(th) * b.x,
                    det::cos(th) * a.y + det::sin(th) * b.y,
                    det::cos(th) * a.z + det::sin(th) * b.z,
                );
                let mut best = f64::INFINITY;
                for n in 0..20_000 {
                    let u = th - 0.01 + 0.02 * f64::from(n) / 20_000.0;
                    let c = Vec3::new(
                        det::cos(u) * x.x + det::sin(u) * y.x,
                        det::cos(u) * x.y + det::sin(u) * y.y,
                        det::cos(u) * x.z + det::sin(u) * y.z,
                    );
                    let d = Vec3::new(c.x - w.x, c.y - w.y, c.z - w.z);
                    best = best.min((d.x * d.x + d.y * d.y + d.z * d.z).sqrt());
                }
                assert!(best <= 5.0 * r, "r {r} direction {j}: {best}");
            }
        }
    }
}
