//! Whether a NURBS surface lies on an analytic surface: the analytic surface's implicit
//! equation composed with the NURBS in homogeneous coordinates is a polynomial in Bernstein
//! form on each Bézier patch, which vanishes identically exactly when the patch lies on the
//! surface. Its coefficients are checked against their rounding (#2388: a support kept as
//! its analytic kind beside its NURBS form, the two refused when they disagree).
//!
//! Not upstream; the fork's `SmBallPatch::IsOnSphere` makes the sphere's check this way.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::{Vec2, Vec3, PI};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::{gamma, NOW};
use crate::nurbs::srf_tool::p1::decompose_to_bez;
use crate::surface::cone::Cone;
use crate::surface::plane::Plane;
use crate::surface::sphere::Sphere;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::torus::Torus;

const OP: &str = "surface::implicit::lies_on";

/// An analytic surface by its implicit equation's data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Implicit {
    /// The plane through `origin` with unit `normal`: `n . X = 0`.
    Plane {
        /// A point of it.
        origin: Vec3,
        /// Its unit normal.
        normal: Vec3,
    },
    /// The cone (a cylinder for `tan_angle = 0`) about the unit `axis` through `origin`,
    /// of radius `radius + tan_angle (X . a)`: `|X - (X . a) a|^2 = (r + t X . a)^2`.
    Cone {
        /// A point of the axis, where the radius is `radius`.
        origin: Vec3,
        /// The unit axis.
        axis: Vec3,
        /// The radius at `origin`.
        radius: f64,
        /// The tangent of the semi-angle.
        tan_angle: f64,
    },
    /// The sphere about `centre`: `|X|^2 = r^2`.
    Sphere {
        /// Its centre.
        centre: Vec3,
        /// Its radius.
        radius: f64,
    },
    /// The torus about the unit `axis` through `centre`:
    /// `(|X|^2 + R^2 - r^2)^2 = 4 R^2 (|X|^2 - (X . a)^2)`.
    Torus {
        /// Its centre.
        centre: Vec3,
        /// The unit axis.
        axis: Vec3,
        /// The major radius R.
        major: f64,
        /// The minor radius r.
        minor: f64,
    },
}

/// A polynomial in Bernstein form over a patch, degrees `(p, q)`, `c[i (q + 1) + j]`.
#[derive(Debug, Clone, PartialEq)]
struct BPoly {
    p: usize,
    q: usize,
    c: Vec<f64>,
}

/// `C(n, k)` as a double.
fn binom(n: usize, k: usize) -> f64 {
    let mut r = 1.0_f64;
    for i in 0..k {
        #[allow(clippy::cast_precision_loss)]
        let f = (n - i) as f64 / (i + 1) as f64;
        r *= f;
    }
    r
}

impl BPoly {
    fn at(&self, i: usize, j: usize) -> f64 {
        self.c.get(i * (self.q + 1) + j).copied().unwrap_or(0.0)
    }

    /// The product, of degrees `(p + p', q + q')`.
    fn mul(&self, o: &Self) -> Self {
        let (p, q) = (self.p + o.p, self.q + o.q);
        let mut c = vec![0.0; (p + 1) * (q + 1)];
        for i in 0..=self.p {
            for j in 0..=self.q {
                for k in 0..=o.p {
                    for l in 0..=o.q {
                        let f = binom(self.p, i) * binom(o.p, k) / binom(p, i + k)
                            * (binom(self.q, j) * binom(o.q, l) / binom(q, j + l));
                        if let Some(x) = c.get_mut((i + k) * (q + 1) + j + l) {
                            *x += f * self.at(i, j) * o.at(k, l);
                        }
                    }
                }
            }
        }
        Self { p, q, c }
    }

    /// `a self + b o`, of the same degrees.
    fn combine(&self, a: f64, o: &Self, b: f64) -> Self {
        Self {
            p: self.p,
            q: self.q,
            c: self
                .c
                .iter()
                .zip(&o.c)
                .map(|(x, y)| a * x + b * y)
                .collect(),
        }
    }

    fn max_abs(&self) -> f64 {
        self.c.iter().fold(0.0_f64, |m, x| m.max(x.abs()))
    }
}

/// Whether `form` lies on `a`: on every Bézier patch, every Bernstein coefficient of the
/// implicit equation in the patch's homogeneous coordinates `(X W, W)` (`X` from the
/// surface's origin) is within the rounding of computing it, `gamma_K` times the size of
/// its terms, `K` four times the operations of the products of the equation's degree d
/// (`K = 4 d (p + 1) (q + 1) + 16`), the stored points a few ulps each.
///
/// # Errors
///
/// `SM_ERR` without NURB data; those of the Bézier decomposition.
pub fn lies_on(form: &BSplineSurface, a: &Implicit) -> Result<bool> {
    let nurb = form.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    let origin = match *a {
        Implicit::Plane { origin, .. } | Implicit::Cone { origin, .. } => origin,
        Implicit::Sphere { centre, .. } | Implicit::Torus { centre, .. } => centre,
    };
    for row in decompose_to_bez(nurb)? {
        for patch in row {
            let (p, q) = patch.degrees();
            let mut x = [Vec::new(), Vec::new(), Vec::new()];
            let mut w = Vec::new();
            for prow in patch.rows() {
                for c in prow {
                    // (stored weighted when rational: H, homogeneous either way)
                    let wi = if c.w == NOW { 1.0 } else { c.w };
                    // X W = H - O W
                    let xw = Vec3::new(c.x, c.y, c.z) - origin * wi;
                    x[0].push(xw.x);
                    x[1].push(xw.y);
                    x[2].push(xw.z);
                    w.push(wi);
                }
            }
            let poly = |c: Vec<f64>| BPoly { p, q, c };
            let [x0, x1, x2] = x;
            let (xs, ws) = ([poly(x0), poly(x1), poly(x2)], poly(w));
            let size = xs.iter().map(BPoly::max_abs).fold(0.0_f64, f64::max);
            let wmax = ws.max_abs();
            let dot = |v: Vec3| xs[0].combine(v.x, &xs[1], v.y).combine(1.0, &xs[2], v.z);
            let norm2 = || {
                xs[0]
                    .mul(&xs[0])
                    .combine(1.0, &xs[1].mul(&xs[1]), 1.0)
                    .combine(1.0, &xs[2].mul(&xs[2]), 1.0)
            };
            let w2 = ws.mul(&ws);
            let (f, scale, degree) = match *a {
                Implicit::Plane { normal, .. } => (dot(normal), size, 1),
                Implicit::Cone {
                    axis,
                    radius,
                    tan_angle,
                    ..
                } => {
                    let along = dot(axis);
                    let r = ws.combine(radius, &along, tan_angle);
                    let f = norm2().combine(1.0, &along.mul(&along), -1.0).combine(
                        1.0,
                        &r.mul(&r),
                        -1.0,
                    );
                    let s = size + (radius + tan_angle.abs() * size) * wmax;
                    (f, s * s, 2)
                }
                Implicit::Sphere { radius, .. } => {
                    let f = norm2().combine(1.0, &w2, -radius * radius);
                    let s = size + radius * wmax;
                    (f, s * s, 2)
                }
                Implicit::Torus {
                    axis, major, minor, ..
                } => {
                    let n2 = norm2();
                    let along = dot(axis);
                    let g = n2.combine(1.0, &w2, major * major - minor * minor);
                    let radial = n2.combine(1.0, &along.mul(&along), -1.0);
                    let f = g
                        .mul(&g)
                        .combine(1.0, &radial.mul(&w2), -4.0 * major * major);
                    let s = size + (major + minor) * wmax;
                    (f, s * s * s * s, 4)
                }
            };
            let tol = gamma(4 * degree * (p + 1) * (q + 1) + 16) * scale;
            let within = |v: &f64| {
                matches!(
                    v.abs().partial_cmp(&tol),
                    Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
                )
            };
            if !f.c.iter().all(within) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

const OP_EXACT: &str = "surface::implicit::check_exact_net";

/// A quadric `x.A.x + 2 b.x + c = 0` as `(A, b, c)`.
pub type QuadricEquation = ([[f64; 3]; 3], [f64; 3], f64);

/// The surface an exact class lies on (#2485), derived from its supports: an analytic
/// surface by its implicit equation (the line, circle and point classes), or the canal
/// surface of a rational spine (the conic, conic-pair and nodal-quartic classes): the
/// points at the ball's radius from the spine, the spine on both faces' offsets.
#[derive(Debug, Clone)]
pub enum ClassSurface {
    /// An analytic surface.
    Implicit(Implicit),
    /// A canal surface of a rational spine.
    Canal {
        /// The spine, the round's centre line.
        spine: Box<BSplineCurve>,
        /// How the net's parameter runs along the spine.
        map: LeaderMap,
        /// The faces' offsets by the ball's radius, which the spine lies on.
        offsets: [QuadricEquation; 2],
        /// The ball's radius.
        radius: f64,
    },
}

/// What an exact net is checked against (#2485): the class its definition names, the
/// class its supports give when classified again, and the surface that class lies on,
/// derived from the supports alone (`None`: none could be).
#[derive(Debug, Clone)]
pub struct ExactNetCheck {
    /// The class the definition names.
    pub class: &'static str,
    /// The class of the supports classified again (`None`: no class kept as a definition).
    pub reclassified: Option<&'static str>,
    /// The class's surface.
    pub surface: Option<ClassSurface>,
}

/// Whether a net is its definition (#2485), the counterpart for a blend of an exact class
/// of the fork's `SmBallPatch::IsOnSphere` and of [`AnalyticSupport::build`]'s check: the
/// supports classified again give the class the definition names, and the net's NURBS form
/// lies on that class's surface ([`lies_on`], each coefficient within the rounding of
/// computing it, no tolerance of its own). Open to more classes: a class adds its label and
/// its implicit surface.
///
/// # Errors
///
/// `InvalidInput` naming the mismatch: the supports of another class (or of none), no
/// surface of the class from them, a net without a NURBS form, or a net off the surface;
/// those of making the net's NURBS form and of [`lies_on`].
pub fn check_exact_net(k: &Kernel, net: &Surface, check: &ExactNetCheck) -> Result<()> {
    if check.reclassified != Some(check.class) {
        return Err(SolidError::invalid_input(
            OP_EXACT,
            "the supports classified again are not of the class the definition names",
        ));
    }
    let Some(surface) = &check.surface else {
        return Err(SolidError::invalid_input(
            OP_EXACT,
            "the supports give the class no surface",
        ));
    };
    let Some(form) = crate::blend::fillet_section::station::as_b_spline(k, net)? else {
        return Err(SolidError::invalid_input(
            OP_EXACT,
            "the net has no NURBS form to check",
        ));
    };
    let on = match surface {
        ClassSurface::Implicit(a) => lies_on(&form, a)?,
        ClassSurface::Canal {
            spine,
            map,
            offsets,
            radius,
        } => {
            if !spine_on_offsets(spine, offsets)? {
                return Err(SolidError::invalid_input(
                    OP_EXACT,
                    "the spine is off the faces' offsets",
                ));
            }
            lies_on_canal(&form, spine, map, *radius)?
        }
    };
    if !on {
        return Err(SolidError::invalid_input(
            OP_EXACT,
            "the net is off the surface of its class",
        ));
    }
    Ok(())
}

/// A Bezier segment of a rational curve, Bernstein polynomials of degree `(p, 0)`: the
/// homogeneous `(Y - O V)` components and the weight `V`.
type CurveSegment = ([BPoly; 3], BPoly);

/// A rational curve's Bezier segments ([`CurveSegment`]), `O` the curve's first point.
fn curve_segments(curve: &BSplineCurve) -> Result<(Vec<CurveSegment>, Vec3)> {
    let nurb = curve
        .nurb
        .as_ref()
        .ok_or_else(|| SolidError::sm_err(OP_EXACT))?;
    let w_of = |w: f64| if w == NOW { 1.0 } else { w };
    let origin = nurb
        .cpts()
        .first()
        .map(|c| Vec3::new(c.x, c.y, c.z) * (1.0 / w_of(c.w)))
        .ok_or_else(|| SolidError::sm_err(OP_EXACT))?;
    let mut out = Vec::new();
    for seg in nurb.decompose_bez()? {
        let p = seg.degree();
        let mut y = [Vec::new(), Vec::new(), Vec::new()];
        let mut v = Vec::new();
        for c in seg.cpts() {
            let wi = w_of(c.w);
            let yw = Vec3::new(c.x, c.y, c.z) - origin * wi;
            y[0].push(yw.x);
            y[1].push(yw.y);
            y[2].push(yw.z);
            v.push(wi);
        }
        let poly = |c: Vec<f64>| BPoly { p, q: 0, c };
        let [y0, y1, y2] = y;
        out.push(([poly(y0), poly(y1), poly(y2)], poly(v)));
    }
    Ok((out, origin))
}

/// Whether the rational curve `spine` lies on both quadrics `offsets`: on every Bezier
/// segment, every Bernstein coefficient of `Y.A.Y + 2 (b + A O).Y V + q(O) V^2` (the
/// quadric about the spine's first point `O`, homogeneous `(Y, V)`) is within the
/// rounding of computing it, `gamma_K` times the size of its terms in the spine's
/// absolute points (the move to `O` is exact algebra; its rounding is theirs), as
/// [`lies_on`].
///
/// # Errors
///
/// `SM_ERR` without NURB data; those of the Bezier decomposition.
fn spine_on_offsets(spine: &BSplineCurve, offsets: &[QuadricEquation; 2]) -> Result<bool> {
    let (segs, o) = curve_segments(spine)?;
    let oa = [o.x, o.y, o.z];
    let dot3 = |x: &[f64; 3], y: &[f64; 3]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f64>();
    for &(a, b, c) in offsets {
        // the quadric about O: A, b' = b + A O, c' = O.A.O + 2 b.O + c
        let ao = a.map(|row| dot3(&row, &oa));
        let b1 = [b[0] + ao[0], b[1] + ao[1], b[2] + ao[2]];
        let c1 = c + dot3(&oa, &ao) + 2.0 * dot3(&b, &oa);
        let amax = a.iter().flatten().fold(0.0_f64, |m, x| m.max(x.abs()));
        let bmax = b.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
        let omax = oa.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
        for (y, v) in &segs {
            let p = v.p;
            let vv = v.mul(v);
            let mut f = vv.combine(c1, &vv, 0.0);
            for (yi, (bi, arow)) in y.iter().zip(b1.iter().zip(a.iter())) {
                f = f.combine(1.0, &yi.mul(v), 2.0 * bi);
                for (yj, aij) in y.iter().zip(arow) {
                    f = f.combine(1.0, &yi.mul(yj), *aij);
                }
            }
            let ys = y.iter().map(BPoly::max_abs).fold(0.0_f64, f64::max);
            let vs = v.max_abs();
            // the terms of the quadric in the spine's own (absolute) homogeneous points: the
            // move to O is exact algebra, its rounding that of these terms
            let ya = ys + omax * vs;
            let scale = amax * ya * ya + 2.0 * bmax * ya * vs + c.abs() * vs * vs;
            let tol = gamma(8 * (p + 1) + 16) * scale;
            if !f.c.iter().all(|x| x.abs() <= tol) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Whether the rational curve `curve` lies on both quadrics `q` (each `X.A.X + 2 b.X + c = 0`),
/// to the rounding of checking it: an exact class's proof for a conic, on its plane and its
/// sphere or elliptic cylinder ([`crate::curve::from_parameters`]).
///
/// # Errors
///
/// `SM_ERR` without NURB data; those of the Bezier decomposition.
pub fn curve_on_quadrics(curve: &BSplineCurve, q: &[QuadricEquation; 2]) -> Result<bool> {
    spine_on_offsets(curve, q)
}

/// How a net's parameter runs along its spine (#2485): the spine's parameter is
/// `a + b w`, `w` the net's u, or its v when `along_v`. Built as the identity in u; the
/// net's reversals, reparametrisations and swaps move it as they move the net.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeaderMap {
    /// The offset a.
    pub a: f64,
    /// The scale b.
    pub b: f64,
    /// Whether the spine runs along the net's v.
    pub along_v: bool,
}

impl Default for LeaderMap {
    fn default() -> Self {
        Self {
            a: 0.0,
            b: 1.0,
            along_v: false,
        }
    }
}

impl LeaderMap {
    /// The net's parameter in `u` (or v) became `w'` with `w = alpha + beta w'`.
    pub fn reparametrize(&mut self, u: bool, alpha: f64, beta: f64) {
        if u != self.along_v {
            self.a += self.b * alpha;
            self.b *= beta;
        }
    }

    /// The net's u and v swapped.
    pub fn swap(&mut self) {
        self.along_v = !self.along_v;
    }
}

/// A homogeneous Bernstein curve `(Y, V)` cut to `[lo, hi]` of its `[0, 1]` (de
/// Casteljau at `hi`, then the left part at `lo / hi`), reversed when `lo > hi`.
fn sub_segment(seg: &CurveSegment, lo: f64, hi: f64) -> CurveSegment {
    let (y, v) = seg;
    let rows: Vec<Vec<f64>> = y
        .iter()
        .map(|c| c.c.clone())
        .chain(core::iter::once(v.c.clone()))
        .collect();
    let split = |rows: &[Vec<f64>], tau: f64, left: bool| -> Vec<Vec<f64>> {
        rows.iter()
            .map(|c| {
                let mut level = c.clone();
                let mut l = Vec::new();
                let mut r = Vec::new();
                while let (Some(&f), Some(&e)) = (level.first(), level.last()) {
                    l.push(f);
                    r.push(e);
                    level = level
                        .windows(2)
                        .map(|w| match w {
                            [x, y] => x * (1.0 - tau) + y * tau,
                            _ => 0.0,
                        })
                        .collect();
                }
                r.reverse();
                if left {
                    l
                } else {
                    r
                }
            })
            .collect()
    };
    let (from, to, reversed) = if lo <= hi {
        (lo, hi, false)
    } else {
        (hi, lo, true)
    };
    let mut cut = if to < 1.0 {
        split(&rows, to, true)
    } else {
        rows
    };
    if from > 0.0 && to > 0.0 {
        cut = split(&cut, from / to, false);
    }
    if reversed {
        for c in &mut cut {
            c.reverse();
        }
    }
    let p = v.p;
    let poly = |c: Vec<f64>| BPoly { p, q: 0, c };
    let mut it = cut.into_iter();
    let mut next = || poly(it.next().unwrap_or_default());
    let ys = [next(), next(), next()];
    (ys, next())
}

/// The breakpoints of a knot vector.
fn breaks(knots: &[f64]) -> Vec<f64> {
    let mut b: Vec<f64> = Vec::new();
    for &x in knots {
        if b.last().is_none_or(|l| *l < x) {
            b.push(x);
        }
    }
    b
}

/// Whether `form` lies on the canal surface of `spine` of radius `radius`: on each of the
/// net's Bezier patches, the spine's piece over the patch's span (through `map`, within one
/// of the spine's segments, cut from it and oriented as the net runs) gives `(Y, V)`, and
/// every Bernstein coefficient of `|X V - W Y|^2 - r^2 W^2 V^2` (homogeneous `(X, W)` of
/// the net, both about the spine's first point) is within the rounding of computing it,
/// `gamma_K` times the size of its terms in the absolute points, as [`lies_on`]. A patch
/// whose span runs over a joint of the spine's segments, or off the spine, is not on it.
///
/// # Errors
///
/// `SM_ERR` without NURB data; those of the Bezier decompositions.
fn lies_on_canal(
    form: &BSplineSurface,
    spine: &BSplineCurve,
    map: &LeaderMap,
    radius: f64,
) -> Result<bool> {
    let nurb = form.gw_nurb().ok_or_else(|| SolidError::sm_err(OP_EXACT))?;
    let (segs, o) = curve_segments(spine)?;
    let spine_breaks = spine
        .nurb
        .as_ref()
        .map(|n| breaks(n.knots()))
        .ok_or_else(|| SolidError::sm_err(OP_EXACT))?;
    let (ub, vb) = (breaks(nurb.u_knots()), breaks(nurb.v_knots()));
    let along = if map.along_v { &vb } else { &ub };
    // the rounding of placing a span on the spine: of a + b w and of the spine's knots
    let span_tol = |s: f64| 8.0 * f64::EPSILON * (map.a.abs() + (map.b * s).abs() + s.abs());
    let patches = decompose_to_bez(nurb)?;
    for (i, row) in patches.iter().enumerate() {
        for (j, patch) in row.iter().enumerate() {
            let k_at = if map.along_v { j } else { i };
            let (Some(&w0), Some(&w1)) = (along.get(k_at), along.get(k_at + 1)) else {
                return Ok(false);
            };
            let (s0, s1) = (map.a + map.b * w0, map.a + map.b * w1);
            let mid = 0.5 * (s0 + s1);
            let Some(m) = spine_breaks.windows(2).position(|w| match w {
                [ka, kb] => *ka <= mid && mid <= *kb,
                _ => false,
            }) else {
                return Ok(false);
            };
            let (Some(&ka), Some(&kb), Some(seg)) =
                (spine_breaks.get(m), spine_breaks.get(m + 1), segs.get(m))
            else {
                return Ok(false);
            };
            let local = |s: f64| -> Option<f64> {
                let t = (s - ka) / (kb - ka);
                let slack = span_tol(s) / (kb - ka);
                (t >= -slack && t <= 1.0 + slack).then(|| t.clamp(0.0, 1.0))
            };
            let (Some(t0), Some(t1)) = (local(s0), local(s1)) else {
                return Ok(false);
            };
            let piece = if t0 == 0.0 && t1 == 1.0 {
                seg.clone()
            } else {
                sub_segment(seg, t0, t1)
            };
            let (y, v) = if map.along_v {
                let turn = |c: &BPoly| BPoly {
                    p: 0,
                    q: c.p,
                    c: c.c.clone(),
                };
                let (ys, vs) = &piece;
                ([turn(&ys[0]), turn(&ys[1]), turn(&ys[2])], turn(vs))
            } else {
                piece
            };
            let (p, q) = patch.degrees();
            let mut x = [Vec::new(), Vec::new(), Vec::new()];
            let mut w = Vec::new();
            for prow in patch.rows() {
                for c in prow {
                    let wi = if c.w == NOW { 1.0 } else { c.w };
                    let xw = Vec3::new(c.x, c.y, c.z) - o * wi;
                    x[0].push(xw.x);
                    x[1].push(xw.y);
                    x[2].push(xw.z);
                    w.push(wi);
                }
            }
            let poly = |c: Vec<f64>| BPoly { p, q, c };
            let [x0, x1, x2] = x;
            let (xs, ws) = ([poly(x0), poly(x1), poly(x2)], poly(w));
            let mut f: Option<BPoly> = None;
            for (xi, yi) in xs.iter().zip(&y) {
                let d = xi.mul(&v).combine(1.0, &ws.mul(yi), -1.0);
                let d2 = d.mul(&d);
                f = Some(match f {
                    None => d2,
                    Some(g) => g.combine(1.0, &d2, 1.0),
                });
            }
            let wv = ws.mul(&v);
            let Some(f) = f else {
                return Ok(false);
            };
            let f = f.combine(1.0, &wv.mul(&wv), -radius * radius);
            // the terms in the absolute homogeneous points (the move to O is exact algebra,
            // its rounding that of these terms)
            let om = o.x.abs().max(o.y.abs()).max(o.z.abs());
            let (wm, vm) = (ws.max_abs(), v.max_abs());
            let xa = xs.iter().map(BPoly::max_abs).fold(0.0_f64, f64::max) + om * wm;
            let ya = y.iter().map(BPoly::max_abs).fold(0.0_f64, f64::max) + om * vm;
            let size = xa * vm + wm * ya + radius * wm * vm;
            let spine_degree = v.p.max(v.q);
            let tol = gamma(8 * (p + spine_degree + 1) * (q + spine_degree + 1) + 16) * size * size;
            if !f.c.iter().all(|c| c.abs() <= tol) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

const OP_BUILD: &str = "surface::implicit::AnalyticSupport::build";
const OP_CHECK: &str = "surface::implicit::AnalyticSupport::check";

/// A support's analytic kind and parameters, beside its NURBS form (#2388): what a
/// record keeps of a plane, cylinder, cone, sphere or torus support.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalyticSupport {
    /// A plane `O + u su X + v sv Y` over its domain.
    Plane {
        /// Its placement.
        placement: Axis2Placement,
        /// `(su, sv)`.
        uv_scale: Vec2,
        /// The domain.
        domain: Extent2d,
    },
    /// A cylinder about the placement's z axis.
    Cylinder {
        /// Its placement.
        placement: Axis2Placement,
        /// The radius.
        radius: f64,
    },
    /// A cone about the placement's z axis.
    Cone {
        /// Its placement.
        placement: Axis2Placement,
        /// The radius at the origin.
        base_radius: f64,
        /// The semi-angle, degrees.
        semi_angle_deg: f64,
    },
    /// A sphere about the placement's origin.
    Sphere {
        /// Its placement.
        placement: Axis2Placement,
        /// The radius.
        radius: f64,
    },
    /// A torus about the placement's z axis.
    Torus {
        /// Its placement.
        placement: Axis2Placement,
        /// The major radius.
        major_radius: f64,
        /// The minor radius.
        minor_radius: f64,
    },
}

impl AnalyticSupport {
    /// The analytic kind of a surface, if it is a plane, cylinder, cone, sphere or torus.
    #[must_use]
    pub fn of(s: &Surface) -> Option<Self> {
        Some(match s.kind() {
            SurfaceKind::Plane(p) => Self::Plane {
                placement: *p.position(),
                uv_scale: p.uv_scale(),
                domain: p.step_uv_domain(),
            },
            SurfaceKind::Cone(c) if c.cylinder => Self::Cylinder {
                placement: c.rev.position,
                radius: c.base_radius,
            },
            SurfaceKind::Cone(c) => Self::Cone {
                placement: c.rev.position,
                base_radius: c.base_radius,
                semi_angle_deg: c.semi_angle_deg,
            },
            SurfaceKind::Sphere(sp) => Self::Sphere {
                placement: sp.base.position,
                radius: sp.radius,
            },
            SurfaceKind::Torus(t) => Self::Torus {
                placement: t.base.position,
                major_radius: t.major_radius,
                minor_radius: t.minor_radius,
            },
            _ => return None,
        })
    }

    /// Its implicit equation's data.
    pub(crate) fn implicit(&self) -> Implicit {
        let axis = |p: &Axis2Placement| p.x_axis().cross(p.y_axis());
        match self {
            Self::Plane { placement, .. } => Implicit::Plane {
                origin: placement.origin(),
                normal: axis(placement),
            },
            Self::Cylinder { placement, radius } => Implicit::Cone {
                origin: placement.origin(),
                axis: axis(placement),
                radius: *radius,
                tan_angle: 0.0,
            },
            Self::Cone {
                placement,
                base_radius,
                semi_angle_deg,
            } => Implicit::Cone {
                origin: placement.origin(),
                axis: axis(placement),
                radius: *base_radius,
                tan_angle: (semi_angle_deg * PI / 180.0).tan(),
            },
            Self::Sphere { placement, radius } => Implicit::Sphere {
                centre: placement.origin(),
                radius: *radius,
            },
            Self::Torus {
                placement,
                major_radius,
                minor_radius,
            } => Implicit::Torus {
                centre: placement.origin(),
                axis: axis(placement),
                major: *major_radius,
                minor: *minor_radius,
            },
        }
    }

    /// An exact class at bound 0 (owner rule 13564): `s`'s analytic kind (its label) with
    /// its NURBS form proved on that kind's implicit surface by [`lies_on`], to the rounding
    /// of checking it. Returns the kind, so a caller can compare it with another surface's
    /// (two kinds equal past the plane's domain are one implicit surface).
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a surface of no analytic kind, without a NURBS form, or whose
    /// form is off its kind; those of making the form and of [`lies_on`].
    pub fn check(k: &Kernel, s: &Surface) -> Result<Self> {
        let a = Self::of(s).ok_or_else(|| {
            SolidError::invalid_input(OP_CHECK, "the surface has no analytic kind")
        })?;
        let form = crate::blend::fillet_section::station::as_b_spline(k, s)?
            .ok_or_else(|| SolidError::invalid_input(OP_CHECK, "the surface has no NURBS form"))?;
        if !lies_on(&form, &a.implicit())? {
            return Err(SolidError::invalid_input(
                OP_CHECK,
                "the surface's NURBS form is off its analytic kind",
            ));
        }
        Ok(a)
    }

    /// The analytic surface on the NURB `form`: refused unless `form` lies on it
    /// ([`lies_on`]) and its placement's axes are orthonormal. A plane keeps its placement
    /// as given; a cylinder, cone, sphere or torus is parametrised as the kernel recognises
    /// it in `form` ([`recognise`]: the sweep angles, generator range, swap and inside-out
    /// are the form's), must be of this support's kind, and carries this support's own
    /// placement, radii and semi-angle on that parametrisation, so that
    /// [`AnalyticSupport::of`] reads the support back unchanged. It must evaluate as `form`
    /// at the form's own parameters ([`evaluates_as`]), which together with [`lies_on`] is
    /// what proves the support and the form agree: [`lies_on`] proves the point set, and
    /// the evaluation proves the parametrisation, including the frame the placement imposes
    /// about the axis and, for a sphere, the whole orientation, none of which the implicit
    /// equation constrains (#2588). The surface takes `form` as its NURB.
    ///
    /// # Errors
    ///
    /// `InvalidInput` where `form` is off the analytic surface or is not its
    /// parametrisation; those of the recognisers, the evaluations and [`lies_on`].
    pub fn build(&self, k: &Kernel, form: BSplineSurface) -> Result<Surface> {
        // the placement's axes as stored: unit and square to rounding (8 ulps)
        let p = match self {
            Self::Plane { placement, .. }
            | Self::Cylinder { placement, .. }
            | Self::Cone { placement, .. }
            | Self::Sphere { placement, .. }
            | Self::Torus { placement, .. } => placement,
        };
        let ulps = 8.0 * f64::EPSILON;
        let unit = |v: Vec3| (v.length() - 1.0).abs() <= ulps;
        let square = p.x_axis().dot(p.y_axis()).abs() <= ulps;
        if !(unit(p.x_axis()) && unit(p.y_axis()) && square) {
            return Err(SolidError::invalid_input(
                OP_BUILD,
                "a support's placement is not orthonormal",
            ));
        }
        if !lies_on(&form, &self.implicit())? {
            return Err(SolidError::invalid_input(
                OP_BUILD,
                "a support's analytic kind disagrees with its NURBS form",
            ));
        }
        if let Self::Plane {
            placement,
            uv_scale,
            domain,
        } = *self
        {
            return Ok(Surface::from(Plane::from_stored(
                placement, uv_scale, domain, form,
            )));
        }
        // a surface of revolution: the one the kernel recognises in the form, which must be
        // this one, take the form as its NURB and evaluate as the form at the form's own
        // parameters
        let not_form = || {
            SolidError::invalid_input(
                OP_BUILD,
                "a support's analytic surface is not its NURBS form's parametrisation",
            )
        };
        let mut surface = recognise(k, self.kind(), &form)?.ok_or_else(not_form)?;
        // the recognised surface must be of this support's kind; the numbers recognition
        // recomputed are not compared with the support's, because it recomputes them from
        // the control points and so reproduces a support's own numbers only to rounding
        // (#2554) -- `lies_on` above and `evaluates_as` below are what prove the two agree
        if Self::of(&surface).map(|a| a.kind()) != Some(self.kind()) {
            return Err(not_form());
        }
        // this support's own numbers on the recognised parametrisation, which the generator
        // and the sweep converter carry, so that `of` reads the support back unchanged
        match (surface.kind_mut(), *self) {
            (SurfaceKind::Cone(c), Self::Cylinder { placement, radius }) => {
                c.rev.position = placement;
                c.base_radius = radius;
                c.rev.base = form.copy();
            }
            (
                SurfaceKind::Cone(c),
                Self::Cone {
                    placement,
                    base_radius,
                    semi_angle_deg,
                },
            ) => {
                c.rev.position = placement;
                c.base_radius = base_radius;
                c.semi_angle_deg = semi_angle_deg;
                c.rev.base = form.copy();
            }
            (SurfaceKind::Sphere(sp), Self::Sphere { placement, radius }) => {
                sp.base.position = placement;
                sp.radius = radius;
                sp.base.base = form.copy();
            }
            (
                SurfaceKind::Torus(t),
                Self::Torus {
                    placement,
                    major_radius,
                    minor_radius,
                },
            ) => {
                t.base.position = placement;
                t.major_radius = major_radius;
                t.minor_radius = minor_radius;
                t.base.base = form.copy();
            }
            _ => return Err(not_form()),
        }
        if !evaluates_as(k, &surface, &form)? {
            return Err(not_form());
        }
        Ok(surface)
    }
}

/// The cylinder, cone, sphere or torus of kind `kind` the kernel recognises in `form`
/// (`Cone::IsNurbSurfaceCone`, `SmSphere::IsNurbSurfaceSphere`,
/// `SmTorus::IsNurbSurfaceTorus`, as `CopyAndAddAnalytics` and the blends' quadrics read
/// them): on the form's frame, sweep and generator ranges and swap. A cone of no angle is
/// a cylinder for the cylinder kind. `None` for none (the recognisers' statuses that find
/// none included).
///
/// # Errors
///
/// [`SolidError::Unported`] and cancellation from the recognisers.
fn recognise(k: &Kernel, kind: SupportKind, form: &BSplineSurface) -> Result<Option<Surface>> {
    let none_on_status = |r: Result<Option<Surface>>| match r {
        Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => Err(e),
        Err(_) => Ok(None),
        Ok(s) => Ok(s),
    };
    match kind {
        SupportKind::Plane => Ok(None),
        SupportKind::Cylinder | SupportKind::Cone => {
            none_on_status(Cone::is_nurb_surface_cone(k, form, 1.0).map(|c| {
                c.map(|mut c| {
                    if kind == SupportKind::Cylinder && c.semi_angle_deg == 0.0 {
                        c.cylinder = true;
                    }
                    Surface::from(c)
                })
            }))
        }
        SupportKind::Sphere => none_on_status(
            Sphere::is_nurb_surface_sphere(k, form, 1.0).map(|s| s.map(Surface::from)),
        ),
        SupportKind::Torus => {
            none_on_status(Torus::is_nurb_surface_torus(k, form, 1.0).map(|t| t.map(Surface::from)))
        }
    }
}

/// Whether `surface` evaluates as `form` at the form's Greville parameters (each control
/// point's), within the rounding of the two evaluations: `gamma_K` times the form's size,
/// `K = 16 (p + q + 2)` for the rational evaluation of degrees `(p, q)` and the analytic
/// one's conversion of the parameters and its trigonometry, a few ulps per operation.
///
/// The analytic side is the **STEP** evaluation of the converted parameters
/// ([`Surface::convert_uv_from_nurbs_to_step`] and [`Surface::evaluate_step_point`]),
/// which a cone, sphere, torus and surface of revolution compute from their placement,
/// radii, semi-angle and generator. [`Surface::evaluate_point`] would not do: a sphere, a
/// torus and a surface of revolution take it from their NURBS base, and that base is the
/// `form` the caller has just installed, so it would compare the form with itself and read
/// nothing of what the caller imposed (#2588; the cone already converted and evaluated
/// analytically, which is why only the other kinds went unchecked).
///
/// # Errors
///
/// Those of the parameter conversion and the evaluations and of the form's knots.
fn evaluates_as(k: &Kernel, surface: &Surface, form: &BSplineSurface) -> Result<bool> {
    let (net, p, q) = euclidean_net(form)?;
    let size = net.iter().flatten().fold(0.0_f64, |a, x| a.max(x.length()));
    let tol = gamma(16 * (p + q + 2)) * size;
    for u in greville(form, true)? {
        for v in greville(form, false)? {
            let uv = Vec2::new(u, v);
            let step_uv = surface.convert_uv_from_nurbs_to_step(k, uv)?;
            let off =
                (surface.evaluate_step_point(k, step_uv)? - form.evaluate_point(uv)?).length();
            if !matches!(
                off.partial_cmp(&tol),
                Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
            ) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

const OP_DERIVE: &str = "surface::implicit::AnalyticSupport::derive";

/// The analytic kind of a support, without its parameters (#2388: a record's label beside
/// the support's NURBS form, which is the support's one geometric truth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportKind {
    /// A plane.
    Plane,
    /// A cylinder.
    Cylinder,
    /// A cone.
    Cone,
    /// A sphere.
    Sphere,
    /// A torus.
    Torus,
}

/// A NURBS surface's control points, Euclidean, `[i along u][j along v]`, with its
/// degrees.
fn euclidean_net(form: &BSplineSurface) -> Result<(Vec<Vec<Vec3>>, usize, usize)> {
    use crate::nurbs::b_spline_surface::SurfParamType;
    let (nu, nv, cps) = form.control_points()?;
    let mut net = vec![Vec::new(); nu];
    for (i, row) in net.iter_mut().enumerate() {
        for j in 0..nv {
            let c = cps
                .get(i * nv + j)
                .ok_or_else(|| SolidError::sm_err(OP_DERIVE))?;
            let w = if c.w == NOW { 1.0 } else { c.w };
            row.push(Vec3::new(c.x, c.y, c.z) * (1.0 / w));
        }
    }
    Ok((
        net,
        form.degree(SurfParamType::U),
        form.degree(SurfParamType::V),
    ))
}

/// The Greville abscissae of a NURBS surface along u (`along_u`) or v: each control
/// point's average of its `degree` inner knots.
fn greville(form: &BSplineSurface, along_u: bool) -> Result<Vec<f64>> {
    use crate::nurbs::b_spline_surface::SurfParamType;
    let dir = if along_u {
        SurfParamType::U
    } else {
        SurfParamType::V
    };
    let p = form.degree(dir);
    let (knots, mults) = form.knots(dir, None)?;
    let mut full = Vec::new();
    for (&k, &m) in knots.iter().zip(&mults) {
        full.extend(std::iter::repeat_n(k, m));
    }
    let n = full.len().saturating_sub(p + 1);
    #[allow(clippy::cast_precision_loss)]
    let pf = p as f64;
    Ok((0..n)
        .map(|i| full.iter().skip(i + 1).take(p).sum::<f64>() / pf)
        .collect())
}

impl AnalyticSupport {
    /// The analytic surface of kind `kind` a NURBS form is, derived from the form: a
    /// plane from its corners (its scales and domain from the knots); a cylinder, cone,
    /// sphere or torus as the kernel recognises it in the form ([`recognise`]: the
    /// recognisers `CopyAndAddAnalytics` runs), on the form's own frame, sweep, generator
    /// range and swap, so that it is the form's parametrisation, not only its point set.
    /// Exact to rounding: a rational NURBS represents these surfaces exactly.
    /// [`AnalyticSupport::build`] then checks the form against the derived surface.
    ///
    /// # Errors
    ///
    /// `InvalidInput` where the form is not of that kind (a degenerate corner, no surface
    /// of the kind recognised in it); those of the control net and the recognisers.
    pub fn derive(k: &Kernel, kind: SupportKind, form: &BSplineSurface) -> Result<Self> {
        let not_kind =
            || SolidError::invalid_input(OP_DERIVE, "a support's NURBS form is not of its kind");
        let (net, p, q) = euclidean_net(form)?;
        let (nu, nv) = (net.len(), net.first().map_or(0, Vec::len));
        let corner = |i: usize, j: usize| net.get(i).and_then(|r| r.get(j)).copied();
        if kind == SupportKind::Plane {
            let (Some(p00), Some(pu), Some(pv)) = (
                corner(0, 0),
                corner(nu.saturating_sub(1), 0),
                corner(0, nv.saturating_sub(1)),
            ) else {
                return Err(not_kind());
            };
            let d = form.natural_uv_domain();
            let (du, dv) = (d.max().x - d.min().x, d.max().y - d.min().y);
            let (eu, ev) = (pu - p00, pv - p00);
            let x = eu * (1.0 / eu.length());
            let ey = ev - x * ev.dot(x);
            let y = ey * (1.0 / ey.length());
            let (su, sv) = (eu.length() / du, ev.dot(y) / dv);
            let origin = p00 - x * (d.min().x * su) - y * (d.min().y * sv);
            let ok = [x, y, origin]
                .iter()
                .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite())
                && su.is_finite()
                && sv.is_finite();
            if !ok {
                return Err(not_kind());
            }
            // the form is the plane's parametrisation: of equal weights (or none), each
            // control point the plane's affine point at its Greville parameters, as a
            // polynomial form is exactly where it reproduces the affine map; otherwise
            // its (u, v) is not the plane's, and it is refused
            let (_, _, cps) = form.control_points()?;
            let w0 = cps.first().map_or(NOW, |c| c.w);
            if cps.iter().any(|c| c.w != w0) {
                return Err(not_kind());
            }
            let (gu, gv) = (greville(form, true)?, greville(form, false)?);
            let size = net
                .iter()
                .flatten()
                .fold(0.0_f64, |a, p| a.max(p.length()))
                .max(origin.length());
            let tol = gamma(8 + p + q) * size;
            for (i, row) in net.iter().enumerate() {
                for (j, pt) in row.iter().enumerate() {
                    let (Some(&u), Some(&v)) = (gu.get(i), gv.get(j)) else {
                        return Err(not_kind());
                    };
                    let affine = origin + x * (u * su) + y * (v * sv);
                    let within = matches!(
                        (*pt - affine).length().partial_cmp(&tol),
                        Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
                    );
                    if !within {
                        return Err(not_kind());
                    }
                }
            }
            return Ok(Self::Plane {
                placement: Axis2Placement::from_stored(origin, x, y),
                uv_scale: Vec2::new(su, sv),
                domain: d,
            });
        }
        // a surface of revolution: the analytic surface the kernel recognises in the form
        // (on the form's own frame, sweep and generator ranges and swap), of the label's
        // kind; `build` checks it evaluates as the form
        recognise(k, kind, form)?
            .as_ref()
            .and_then(Self::of)
            .filter(|a| a.kind() == kind)
            .ok_or_else(not_kind)
    }

    /// Not a support's kind (#2485): the surface of revolution a NURBS form is, as the
    /// kernel recognises it in the form (`IsNurbSurfaceSurfOfRevolution`, which
    /// `CopyAndAddAnalytics` runs), on the form's own frame, sweep, generator and swap;
    /// it takes `form` as its NURB and must evaluate as the form at the form's own
    /// parameters, as [`AnalyticSupport::build`] requires of a support.
    ///
    /// # Errors
    ///
    /// `InvalidInput` where the form is no surface of revolution or is not the recognised
    /// one's parametrisation; those of the recogniser and the evaluations.
    pub fn derive_revolution(k: &Kernel, form: &BSplineSurface) -> Result<Surface> {
        use crate::surface::surf_of_revolution::SurfOfRevolution;
        let not_kind = || {
            SolidError::invalid_input(
                OP_DERIVE,
                "an exact net's NURBS form is not a surface of revolution's parametrisation",
            )
        };
        let test = Surface::from(form.copy());
        let mut rev = match SurfOfRevolution::is_nurb_surface_surf_of_revolution(k, &test, 1.0) {
            Ok(Some(rev)) => rev,
            Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => return Err(e),
            Ok(None) | Err(_) => return Err(not_kind()),
        };
        rev.base = form.copy();
        let surface = Surface::from(rev);
        if !evaluates_as(k, &surface, form)? {
            return Err(not_kind());
        }
        Ok(surface)
    }

    /// Its kind, without its parameters.
    #[must_use]
    pub const fn kind(&self) -> SupportKind {
        match self {
            Self::Plane { .. } => SupportKind::Plane,
            Self::Cylinder { .. } => SupportKind::Cylinder,
            Self::Cone { .. } => SupportKind::Cone,
            Self::Sphere { .. } => SupportKind::Sphere,
            Self::Torus { .. } => SupportKind::Torus,
        }
    }
}
