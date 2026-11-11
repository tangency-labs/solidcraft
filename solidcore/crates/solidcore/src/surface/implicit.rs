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
use crate::geom::vector2d::Point2;
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

impl Implicit {
    /// How far `x` is from this surface.
    #[must_use]
    pub fn distance(&self, x: Vec3) -> f64 {
        // the axial and radial parts of `x` about an axis through `o`
        let about = |o: Vec3, axis: Vec3| {
            let v = x - o;
            let h = v.dot(axis);
            (h, (v - axis * h).length())
        };
        match *self {
            Self::Plane { origin, normal } => (x - origin).dot(normal).abs(),
            Self::Cone {
                origin,
                axis,
                radius,
                tan_angle,
            } => {
                let (h, rho) = about(origin, axis);
                (rho - (radius + tan_angle * h)).abs() / (1.0 + tan_angle * tan_angle).sqrt()
            }
            Self::Sphere { centre, radius } => ((x - centre).length() - radius).abs(),
            Self::Torus {
                centre,
                axis,
                major,
                minor,
            } => {
                let (h, rho) = about(centre, axis);
                let (a, b) = (rho - major, h);
                ((a * a + b * b).sqrt() - minor).abs()
            }
        }
    }
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
/// (`K = 4 d (p + 1) (q + 1) + 16`), the stored points a few ulps each. One factor of that
/// size is the absolute one, `|H| + |O| W`: the points are stored, and moved to `O`, in
/// absolute coordinates, so a patch far from the origin is known only to the rounding there,
/// however small it is (a 1.8 mm hole at (150, 150) holds its points to ulp(150)).
///
/// # Errors
///
/// `SM_ERR` without NURB data; those of the Bézier decomposition.
pub fn lies_on(form: &BSplineSurface, a: &Implicit) -> Result<bool> {
    let nurb = form.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    for row in decompose_to_bez(nurb)? {
        for patch in row {
            let (p, q) = patch.degrees();
            if !bezier_on(p, q, patch.rows().flatten(), a) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Whether the rational curve `curve` lies on `a`, as [`lies_on`] checks it of a surface:
/// on every Bézier segment (a patch of degrees `(p, 0)`), every Bernstein coefficient of the
/// implicit equation is within the rounding of computing it from the segment's control
/// points as they are stored. Nothing is sampled.
///
/// What it shows is that the segments' points are on `a` to the rounding of evaluating
/// the equation at part scale, not exact containment: the bound does not cover the
/// rounding of the Bézier decomposition (its knot insertion), nor of any trim or
/// construction that made `curve`; a curve moved off `a` by more than that rounding is
/// refused.
///
/// # Errors
///
/// `SM_ERR` without NURB data; those of the Bézier decomposition.
pub fn curve_lies_on(curve: &BSplineCurve, a: &Implicit) -> Result<bool> {
    let nurb = curve.nurb.as_ref().ok_or_else(|| SolidError::sm_err(OP))?;
    for seg in nurb.decompose_bez()? {
        if !bezier_on(seg.degree(), 0, seg.cpts().iter(), a) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The plane `form` lies on, certified by [`lies_on`]: through its first control point,
/// normal to the two of its control points' chords from there that span the most (the
/// farthest point, then the one farthest off that chord), so the normal is as well
/// conditioned as the net allows. `None` where the net spans no plane (its points on one
/// line) or does not lie on that one: a net off a plane by more than the rounding of its
/// stored points is no plane.
///
/// # Errors
///
/// `SM_ERR` without NURB data; those of [`lies_on`].
pub fn plane_of_net(form: &BSplineSurface) -> Result<Option<Implicit>> {
    let (net, _, _) = euclidean_net(form)?;
    let points: Vec<Vec3> = net.into_iter().flatten().collect();
    let Some(&origin) = points.first() else {
        return Ok(None);
    };
    let far = |a: &&Vec3, b: &&Vec3| (**a - origin).length().total_cmp(&(**b - origin).length());
    let Some(&along) = points.iter().max_by(far) else {
        return Ok(None);
    };
    let chord = along - origin;
    let off = |a: &&Vec3, b: &&Vec3| {
        let (x, y) = (chord.cross(**a - origin), chord.cross(**b - origin));
        x.length().total_cmp(&y.length())
    };
    let Some(&across) = points.iter().max_by(off) else {
        return Ok(None);
    };
    let Ok(normal) = chord.cross(across - origin).unitize() else {
        return Ok(None);
    };
    let plane = Implicit::Plane { origin, normal };
    Ok(lies_on(form, &plane)?.then_some(plane))
}

/// Whether the Bézier patch of degrees `(p, q)` with homogeneous points `points` (row by
/// row) lies on `a` ([`lies_on`]).
fn bezier_on<'a>(
    p: usize,
    q: usize,
    points: impl Iterator<Item = &'a crate::nurbs::math::CPoint>,
    a: &Implicit,
) -> bool {
    let origin = match *a {
        Implicit::Plane { origin, .. } | Implicit::Cone { origin, .. } => origin,
        Implicit::Sphere { centre, .. } | Implicit::Torus { centre, .. } => centre,
    };
    let mut x = [Vec::new(), Vec::new(), Vec::new()];
    let mut w = Vec::new();
    // the largest absolute homogeneous term of the move to O, `|H| + |O| W`: the
    // stored points are known to their rounding at this size, and the move rounds
    // at it, however near O they lie
    let mut absolute = 0.0_f64;
    let om = origin.x.abs().max(origin.y.abs()).max(origin.z.abs());
    for c in points {
        // (stored weighted when rational: H, homogeneous either way)
        let wi = if c.w == NOW { 1.0 } else { c.w };
        let hm = c.x.abs().max(c.y.abs()).max(c.z.abs());
        absolute = absolute.max(hm + om * wi.abs());
        // X W = H - O W
        let xw = Vec3::new(c.x, c.y, c.z) - origin * wi;
        x[0].push(xw.x);
        x[1].push(xw.y);
        x[2].push(xw.z);
        w.push(wi);
    }
    let poly = |c: Vec<f64>| BPoly { p, q, c };
    let [x0, x1, x2] = x;
    let (xs, ws) = ([poly(x0), poly(x1), poly(x2)], poly(w));
    let size = xs.iter().map(BPoly::max_abs).fold(0.0_f64, f64::max);
    let absolute = absolute.max(size);
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
        Implicit::Plane { normal, .. } => (dot(normal), absolute, 1),
        Implicit::Cone {
            axis,
            radius,
            tan_angle,
            ..
        } => {
            let along = dot(axis);
            let r = ws.combine(radius, &along, tan_angle);
            let f = norm2()
                .combine(1.0, &along.mul(&along), -1.0)
                .combine(1.0, &r.mul(&r), -1.0);
            let s = size + (radius + tan_angle.abs() * size) * wmax;
            let sa = absolute + (radius + tan_angle.abs() * absolute) * wmax;
            (f, s * sa, 2)
        }
        Implicit::Sphere { radius, .. } => {
            let f = norm2().combine(1.0, &w2, -radius * radius);
            let s = size + radius * wmax;
            (f, s * (absolute + radius * wmax), 2)
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
            (f, s * s * s * (absolute + (major + minor) * wmax), 4)
        }
    };
    let tol = gamma(4 * degree * (p + 1) * (q + 1) + 16) * scale;
    let within = |v: &f64| {
        matches!(
            v.abs().partial_cmp(&tol),
            Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
        )
    };
    f.c.iter().all(within)
}

pub(crate) const OP_EXACT: &str = "surface::implicit::check_exact_net";

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

/// The number of intervals [`net_miss`] samples a net at, each way.
const MISS_SAMPLES: u32 = 16;

/// How far `net` strays from the class's `surface`, sampled over its domain. A guard, not a
/// certificate: [`check_exact_net`] is the proof, and a net that passes only this keeps no
/// definition. It tells a net that the class's rounding leaves off the surface from one
/// built wrongly. Being sampled ([`MISS_SAMPLES`] intervals each way), it can miss a
/// deviation between its samples; it bounds nothing, and decides only whether a net its
/// certificate refused is kept bare or is an error.
///
/// # Errors
///
/// Those of the evaluations.
pub fn net_miss(k: &Kernel, net: &Surface, surface: &ClassSurface) -> Result<f64> {
    let dom = net.natural_uv_domain_with_nurb(k)?;
    let (lo, hi) = (dom.min(), dom.max());
    let mut worst = 0.0_f64;
    for i in 0..=MISS_SAMPLES {
        for j in 0..=MISS_SAMPLES {
            let (fu, fv) = (
                f64::from(i) / f64::from(MISS_SAMPLES),
                f64::from(j) / f64::from(MISS_SAMPLES),
            );
            let uv = Point2::new(lo.x + (hi.x - lo.x) * fu, lo.y + (hi.y - lo.y) * fv);
            let x = net.evaluate_point(k, uv)?;
            let miss = match surface {
                ClassSurface::Implicit(a) => a.distance(x),
                ClassSurface::Canal {
                    spine, map, radius, ..
                } => {
                    let w = if map.along_v { uv.y } else { uv.x };
                    let ivl = spine.natural_interval();
                    let s = (map.a + map.b * w).max(ivl.min()).min(ivl.max());
                    ((x - spine.evaluate_point(s)?).length() - *radius).abs()
                }
            };
            worst = worst.max(miss);
        }
    }
    Ok(worst)
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

/// How many halvings [`conic_turn`] makes of one Bézier segment before it gives up: a
/// segment whose angular speed it cannot prove positive after this many is refused.
const TURN_DEPTH: usize = 24;

/// The angle, degrees, that the rational curve `curve` turns about the conic of centre `o`,
/// unit axes `x`, `y` and radii `rx`, `ry` (its angle `atan2((P - o).y / ry, (P - o).x / rx)`),
/// certified anticlockwise, or `None` where it cannot be: for a curve proved on the conic
/// ([`curve_on_quadrics`]), the sweep of its parameter over the conic.
///
/// On each Bézier segment, homogeneous `(Xh, Yh, W)` in the conic's scaled frame, the sign
/// of the angular speed is that of `N = Xh Yh' - Yh Xh'` (the weight's terms cancel:
/// `dθ/dt = N / (Xh² + Yh²)`), a polynomial in Bernstein form whose coefficients are
/// computed with their rounding bound (`gamma_K` times their absolute terms). The segment
/// turns strictly anticlockwise when every coefficient is above its bound (the first and
/// last may be zero to rounding: a speed vanishing at a segment's end). Where the
/// coefficients do not decide, the segment is halved (de Casteljau), at most
/// [`TURN_DEPTH`] times; one coefficient below minus its bound everywhere is a turn back,
/// refused at once. A segment so proved and whose control points (all weights positive)
/// lie in the open half plane its chord's bisector faces turns less than half a turn, so
/// its angle is exactly that between its end points, and the total is the sum of these.
/// Nothing is sampled.
///
/// # Errors
///
/// `SM_ERR` without NURB data; those of the Bézier decomposition.
pub fn conic_turn(
    curve: &BSplineCurve,
    o: Vec3,
    x: Vec3,
    y: Vec3,
    rx: f64,
    ry: f64,
) -> Result<Option<f64>> {
    let (segs, first) = curve_segments(curve)?;
    // the move from the curve's first point to the conic's centre, and its size
    let shift = first - o;
    let mut total = 0.0_f64;
    for seg in segs {
        let mut todo = vec![(seg, 0_usize)];
        while let Some((piece, depth)) = todo.pop() {
            match piece_turn(&piece, shift, x, y, rx, ry) {
                Turn::Anticlockwise(deg) => total += deg,
                Turn::Back => return Ok(None),
                Turn::Undecided => {
                    if depth >= TURN_DEPTH {
                        return Ok(None);
                    }
                    todo.push((sub_segment(&piece, 0.0, 0.5), depth + 1));
                    todo.push((sub_segment(&piece, 0.5, 1.0), depth + 1));
                }
            }
        }
    }
    Ok(Some(total))
}

/// What [`piece_turn`] proves of one Bézier piece.
enum Turn {
    /// It turns anticlockwise by this many degrees, less than half a turn.
    Anticlockwise(f64),
    /// It turns back somewhere, or a weight is not positive.
    Back,
    /// Its coefficients do not decide.
    Undecided,
}

/// The derivative of a Bernstein polynomial of degree `p >= 1` in one variable, degree
/// `p - 1`, and of its absolute terms `a` (`p (|c_{i+1}| + |c_i|)`).
fn derivative(c: &[f64], a: &[f64]) -> (BPoly, BPoly) {
    let p = c.len().saturating_sub(1);
    #[allow(clippy::cast_precision_loss)]
    let pf = p as f64;
    let step = |v: &[f64], absolute: bool| -> Vec<f64> {
        v.iter()
            .zip(v.iter().skip(1))
            .map(|(l, r)| {
                if absolute {
                    pf * (l.abs() + r.abs())
                } else {
                    pf * (r - l)
                }
            })
            .collect()
    };
    let q = p.saturating_sub(1);
    (
        BPoly {
            p: q,
            q: 0,
            c: step(c, false),
        },
        BPoly {
            p: q,
            q: 0,
            c: step(a, true),
        },
    )
}

/// [`conic_turn`] on one Bézier piece `(Y, W)` (about the curve's first point, `shift` from
/// it to the conic's centre).
fn piece_turn(piece: &CurveSegment, shift: Vec3, x: Vec3, y: Vec3, rx: f64, ry: f64) -> Turn {
    let (ys, w) = piece;
    if w.p == 0 || !w.c.iter().all(|wi| *wi > 0.0) {
        return Turn::Back;
    }
    // Xh = (Y + shift W) . x / rx, Yh likewise, and their absolute terms
    let n = w.c.len();
    let at = |axis: Vec3, r: f64, absolute: bool| -> Vec<f64> {
        (0..n)
            .map(|i| {
                let yi = Vec3::new(
                    ys[0].c.get(i).copied().unwrap_or(0.0),
                    ys[1].c.get(i).copied().unwrap_or(0.0),
                    ys[2].c.get(i).copied().unwrap_or(0.0),
                );
                let wi = w.c.get(i).copied().unwrap_or(0.0);
                if absolute {
                    let a = Vec3::new(axis.x.abs(), axis.y.abs(), axis.z.abs());
                    let ya = Vec3::new(yi.x.abs(), yi.y.abs(), yi.z.abs());
                    let sa = Vec3::new(shift.x.abs(), shift.y.abs(), shift.z.abs());
                    (ya + sa * wi).dot(a) / r
                } else {
                    (yi + shift * wi).dot(axis) / r
                }
            })
            .collect()
    };
    let (xh, yh) = (at(x, rx, false), at(y, ry, false));
    let (xa, ya) = (at(x, rx, true), at(y, ry, true));
    let p = w.p;
    let poly = |c: Vec<f64>| BPoly { p, q: 0, c };
    let (dx, dxa) = derivative(&xh, &xa);
    let (dy, dya) = derivative(&yh, &ya);
    let num = poly(xh.clone())
        .mul(&dy)
        .combine(1.0, &poly(yh.clone()).mul(&dx), -1.0);
    let size = poly(xa.clone())
        .mul(&dya)
        .combine(1.0, &poly(ya.clone()).mul(&dxa), 1.0);
    let k = 16 * (2 * p + 2) + 32;
    let bound = |i: usize| gamma(k) * size.c.get(i).copied().unwrap_or(0.0);
    let last = num.c.len().saturating_sub(1);
    let mut decided = true;
    for (i, c) in num.c.iter().enumerate() {
        let b = bound(i);
        let end = i == 0 || i == last;
        let positive = if end { *c >= -b } else { *c > b };
        if !positive {
            decided = false;
        }
    }
    if num.c.iter().enumerate().all(|(i, c)| *c < -bound(i)) {
        return Turn::Back;
    }
    if !decided {
        return Turn::Undecided;
    }
    // the half plane the chord's bisector m faces: every control point, (Xh, Yh) / W with
    // W > 0, strictly in it, so the piece turns less than half a turn
    let (Some(&w0), Some(&w1)) = (w.c.first(), w.c.last()) else {
        return Turn::Undecided;
    };
    let (Some(&x0), Some(&y0), Some(&x1), Some(&y1)) =
        (xh.first(), yh.first(), xh.last(), yh.last())
    else {
        return Turn::Undecided;
    };
    let (sx, sy, ex, ey) = (x0 / w0, y0 / w0, x1 / w1, y1 / w1);
    let (mx, my) = (sx + ex, sy + ey);
    let inside = xh
        .iter()
        .zip(&yh)
        .zip(xa.iter().zip(&ya))
        .all(|((cx, cy), (ax, ay))| mx * cx + my * cy > gamma(k) * (mx.abs() * ax + my.abs() * ay));
    if !inside {
        return Turn::Undecided;
    }
    Turn::Anticlockwise((sx * ey - sy * ex).atan2(sx * ex + sy * ey).to_degrees())
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

    /// Whether `s`'s form proves its analytic label ([`Self::check`]'s test): true where it
    /// has no label or no form, so nothing to prove.
    ///
    /// # Errors
    ///
    /// Those of making the form and of [`lies_on`].
    pub fn label_stands(k: &Kernel, s: &Surface) -> Result<bool> {
        let Some(a) = Self::of(s) else {
            return Ok(true);
        };
        match crate::blend::fillet_section::station::as_b_spline(k, s)? {
            Some(form) => lies_on(&form, &a.implicit()),
            None => Ok(true),
        }
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
        let surface = self.on_form(k, &form)?;
        if !evaluates_as(k, &surface, &form)? {
            return Err(not_form());
        }
        Ok(surface)
    }

    /// The largest ratio, over the form's Greville points, of this support's analytic
    /// surface's distance from `form` to the rounding [`evaluates_as`] derives there (#3185):
    /// at most 1 where [`Self::build`]'s parametrisation test passes. The surface is the one
    /// `build` makes (the kernel's recognition of `form`, carrying this support's numbers);
    /// `lies_on` and the placement's axes are not checked here. `None` for a plane, or a
    /// surface the bound does not cover.
    ///
    /// # Errors
    ///
    /// `InvalidInput` where the kernel recognises no surface of this kind in `form`; those of
    /// the recognisers, the conversions and the evaluations.
    pub fn parametrisation_margin(&self, k: &Kernel, form: &BSplineSurface) -> Result<Option<f64>> {
        if matches!(self, Self::Plane { .. }) {
            return Ok(None);
        }
        let surface = self.on_form(k, form)?;
        evaluation_margin(k, &surface, form)
    }

    /// The surface of revolution of this support's kind the kernel recognises in `form`,
    /// carrying this support's own numbers and `form` as its NURB ([`Self::build`] after its
    /// checks of the point set, before the parametrisation's).
    fn on_form(&self, k: &Kernel, form: &BSplineSurface) -> Result<Surface> {
        let form = form.copy();
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
                on_axis_from(&mut c.rev, placement)?;
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
                on_axis_from(&mut c.rev, placement)?;
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
        Ok(surface)
    }
}

/// [`AnalyticSupport::build`]'s refusal of a surface that is not its form's parametrisation.
fn not_form() -> SolidError {
    SolidError::invalid_input(
        OP_BUILD,
        "a support's analytic surface is not its NURBS form's parametrisation",
    )
}

/// A cylinder's or cone's recognised parametrisation carried by the support's own
/// `placement`: its heights are measured along the axis from the placement's origin, so the
/// recognised heights move by how far along the axis the recognised origin lies from it.
/// The record puts a frame's origin anywhere on the axis (a hole's at the solid's floor, its
/// form from the face's own bottom); the point at each parameter is then unchanged, which
/// [`evaluates_as`] proves. A placement off the axis or turned about it is not the form's
/// parametrisation, and is refused there.
///
/// The generator line's parameters are the heights too, so it moves with them: its STEP
/// interval stays the surface's v interval, as the kernel's check requires
/// (`SmSurfOfRevolution::Test` #2, `SmCone::Test` #1), and its points stay where they are.
fn on_axis_from(
    rev: &mut crate::surface::surf_of_revolution::SurfOfRevolution,
    placement: Axis2Placement,
) -> Result<()> {
    use crate::curve::curve::CurveKind;
    let h = (rev.position.origin() - placement.origin()).dot(placement.z_axis());
    let d = rev.anal_uv_domain;
    rev.anal_uv_domain = Extent2d::from_bounds(d.u_min(), d.v_min() + h, d.u_max(), d.v_max() + h);
    rev.position = placement;
    match rev
        .gen_curve
        .as_mut()
        .map(crate::curve::curve::Curve::kind_mut)
    {
        Some(CurveKind::Line(line)) => {
            line.shift_step_parameter(h);
            Ok(())
        }
        _ => Err(SolidError::invalid_input(
            OP_BUILD,
            "a recognised cylinder or cone has no generator line",
        )),
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
/// point's): at each, the analytic surface's point and the form's agree within the bound
/// [`Rounding::at`] derives for that point, term by term below. The analytic side is the
/// **STEP** evaluation of the converted parameters ([`Surface::convert_uv_from_nurbs_to_step`]
/// and [`Surface::evaluate_step_point`]), which a cone, sphere, torus and surface of
/// revolution compute from their placement, radii, semi-angle and generator.
/// [`Surface::evaluate_point`] would not do: a sphere, a torus and a surface of revolution
/// take it from their NURBS base, and that base is the `form` the caller has just installed,
/// so it would compare the form with itself and read nothing of what the caller imposed
/// (#2588).
///
/// What the bound forgives and what it tests (#3185): it forgives the rounding of both
/// evaluations and of the two parameter conversions, nothing else. It does not forgive the
/// label's frame turned about the axis against the converter's, its axis, origin or radii,
/// a form whose parameter is not the analytic surface's (a reparametrised arc), nor the
/// form's distance from the analytic surface: those are what the comparison proves.
///
/// `u` is the unit roundoff, `gamma_n = n u / (1 - n u)`. Stated assumption: the libm
/// `sin`, `cos`, `tan` and `atan2` are within one ulp (relative `2 u`).
///
/// 1. **The form** (`Surface::eval`, Cox-de Boor then two blends): each basis value is a
///    nonnegative combination over `p` levels of at most 6 operations (two knot differences,
///    a sum, a division, a product and a sum), so relative `gamma_6p`; the blends sum
///    `q + 1` then `p + 1` products. With positive weights the homogeneous point's error is
///    at most `gamma_(7 (p + q) + 4) W size` and the weight's `gamma_(7 (p + q) + 4) W`, so
///    the Euclidean point's, after the division, `E_form = (2 gamma_(7 (p + q) + 4) + u)
///    size` (`gamma_(7 (p + q) + 4) size` when polynomial), `size` the net's largest point.
/// 2. **The STEP point**, `O + m (cos u X + sin u Y) + h Z`, `m` and `h` the generator's
///    radial and axial parts (the torus `R + r cos v`, `r sin v`; the sphere `r cos v`,
///    `r sin v`; the cone `r0 + v tan a`, `v`). An angle `x` in degrees goes to radians in
///    two operations of the rounded `PI`: `gamma_3 |x|`; with one ulp of the cosine and sine,
///    each is off by `dc(x) = 2 u + gamma_3 |x|`. Per coordinate (unit axes) the error is at
///    most `gamma_3 (|O| + M + H) + M (gamma_2 + 2 dc(u)) + dm + dh`, `M`, `H` the sizes of
///    `m`, `h` and `dm`, `dh` their errors: the torus `gamma_3 (R + r) + 2 r dc(v)`, the
///    sphere `gamma_1 r + 2 r dc(v)`, the cone `gamma_2 (|r0| + |v t|) + |v| dt` (`dt =
///    (1 + t^2) gamma_3 |a| + 2 u |t|` for the tangent of the half angle `a`); a general
///    surface of revolution rotates its generator's point `G` (a B-spline: item 1's bound of
///    its own net, `E_gen`) about the axis, `gamma_8 (|O| + 3 D) + 2 D dc(u) + E_gen`, `D`
///    the net's reach from `O`. `E_step` is `sqrt 3` times the coordinate bound.
/// 3. **The generator's parameter**: a circle (torus, sphere) inverts its NURBS point by
///    `atan2` about its centre `c`: at its radius `r` the error is that point's (`E_form`)
///    plus `gamma_4 (size + |c|)` and `gamma_4 r |v|` of the angle's own rounding; a line
///    (cone) maps it affinely in 6 operations, `gamma_6 2 (|v0| + |v1|)` along the axis and
///    its slope, times `1 + |t|`; a B-spline generator's parameter is its NURBS's (none).
/// 4. **The sweep's parameter**: the polar converter evaluates its circle's NURBS at the
///    parameter (item 1's bound of that NURBS's own net, `E_polar`) and inverts the point by
///    `atan2` about the circle's centre `c` in its frame: at the circle's radius `rc` the
///    angle is off by `(E_polar + gamma_4 (rc + 2 |c|)) / rc`, plus `gamma_4 |s|` of the
///    angle's own rounding and the degrees' period (`gamma_1 2 pi`), times the STEP point's
///    distance from that axis (the lever). The converter's ends are its domain's, which
///    recognition takes from the form's own end meridians (#3199), so they are the inverted
///    ends to the same rounding.
///
/// A surface of another kind, or a general surface of revolution whose generator is not a
/// B-spline, has no bound here and is not proved.
///
/// # Errors
///
/// Those of the parameter conversion and the evaluations and of the form's knots.
fn evaluates_as(k: &Kernel, surface: &Surface, form: &BSplineSurface) -> Result<bool> {
    Ok(evaluation_margin(k, surface, form)?.is_some_and(|m| m <= 1.0))
}

/// [`evaluates_as`]'s largest ratio of distance to bound over the Greville points (infinite
/// for a distance that is not a number); `None` for a surface the bound does not cover.
fn evaluation_margin(k: &Kernel, surface: &Surface, form: &BSplineSurface) -> Result<Option<f64>> {
    let (net, p, q) = euclidean_net(form)?;
    let size = net.iter().flatten().fold(0.0_f64, |a, x| a.max(x.length()));
    let Some(r) = Rounding::of(surface, form.is_rational(), p, q, size, &net)? else {
        return Ok(None);
    };
    let mut worst = 0.0_f64;
    for u in greville(form, true)? {
        for v in greville(form, false)? {
            let uv = Vec2::new(u, v);
            let step_uv = surface.convert_uv_from_nurbs_to_step(k, uv)?;
            let s = surface.evaluate_step_point(k, step_uv)?;
            let f = form.evaluate_point(uv)?;
            let ratio = (s - f).length() / r.at(step_uv, s, f);
            worst = if ratio.is_nan() {
                f64::INFINITY
            } else {
                worst.max(ratio)
            };
        }
    }
    Ok(Some(worst))
}

/// A surface of revolution's generator, for [`Rounding`]'s item 2 and 3.
#[derive(Debug, Clone, Copy)]
enum Generator {
    /// `R + r cos v`, `r sin v` about the circle of centre `c`.
    Torus { major: f64, minor: f64, c: f64 },
    /// `r cos v`, `r sin v` about the circle of centre `c`.
    Sphere { radius: f64, c: f64 },
    /// `r0 + v t`, `v` (the half angle `a` in radians), its STEP `v` over `[v0, v1]`.
    Cone {
        r0: f64,
        t: f64,
        a: f64,
        v0: f64,
        v1: f64,
    },
    /// A B-spline generator, its own evaluation's bound.
    Spline { e_gen: f64 },
}

/// The pieces of [`evaluates_as`]'s bound that do not depend on the point.
#[derive(Debug, Clone, Copy)]
struct Rounding {
    e_form: f64,
    origin: Vec3,
    /// The net's reach from the origin.
    reach: f64,
    generator: Generator,
    /// The sweep converter's circle: centre, axes.
    polar: (Vec3, Vec3, Vec3, f64, f64),
}

/// `|v|`'s largest coordinate.
fn max_coord(v: Vec3) -> f64 {
    v.x.abs().max(v.y.abs()).max(v.z.abs())
}

/// A rational (or polynomial) evaluation's bound of degrees summing to `n` over points of
/// size `size` ([`evaluates_as`] item 1).
fn eval_bound(rational: bool, n: usize, size: f64) -> f64 {
    let g = gamma(7 * n + 4);
    if rational {
        (2.0 * g + crate::nurbs::math::UNIT) * size
    } else {
        g * size
    }
}

impl Rounding {
    /// The bound's data for `surface` (a cone, sphere, torus or surface of revolution with a
    /// B-spline generator) on its form of degrees `(p, q)`, net `net`; `None` for any other.
    fn of(
        surface: &Surface,
        rational: bool,
        p: usize,
        q: usize,
        size: f64,
        net: &[Vec<Vec3>],
    ) -> Result<Option<Self>> {
        use crate::curve::curve::CurveKind;
        let e_form = eval_bound(rational, p + q, size);
        let circle_centre = |rev: &crate::surface::surf_of_revolution::SurfOfRevolution| match rev
            .gen_curve
            .as_ref()
            .map(crate::curve::curve::Curve::kind)
        {
            Some(CurveKind::Ellipse(e)) => Some(e.position.origin().length()),
            _ => None,
        };
        let (rev, generator) = match surface.kind() {
            SurfaceKind::Torus(t) => {
                let Some(c) = circle_centre(&t.base) else {
                    return Ok(None);
                };
                (
                    &t.base,
                    Generator::Torus {
                        major: t.major_radius,
                        minor: t.minor_radius,
                        c,
                    },
                )
            }
            SurfaceKind::Sphere(sp) => {
                let Some(c) = circle_centre(&sp.base) else {
                    return Ok(None);
                };
                (
                    &sp.base,
                    Generator::Sphere {
                        radius: sp.radius,
                        c,
                    },
                )
            }
            SurfaceKind::Cone(c) => {
                let a = c.semi_angle_deg * PI / 180.0;
                let d = c.rev.anal_uv_domain;
                (
                    &c.rev,
                    Generator::Cone {
                        r0: c.base_radius,
                        t: c.tan_semi_angle(),
                        a,
                        v0: d.v_min(),
                        v1: d.v_max(),
                    },
                )
            }
            SurfaceKind::SurfOfRevolution(rev) => {
                let Some(CurveKind::BSpline(g)) =
                    rev.gen_curve.as_ref().map(crate::curve::curve::Curve::kind)
                else {
                    return Ok(None);
                };
                let pts = g.control_points()?;
                let gsize = pts.iter().fold(0.0_f64, |m, c| {
                    let w = if c.w == NOW { 1.0 } else { c.w };
                    m.max((Vec3::new(c.x, c.y, c.z) * (1.0 / w)).length())
                });
                let rational = pts.iter().any(|c| c.w != NOW);
                (
                    &**rev,
                    Generator::Spline {
                        e_gen: eval_bound(rational, g.degree()?, gsize),
                    },
                )
            }
            _ => return Ok(None),
        };
        let Some(pc) = rev.polar_curve() else {
            return Ok(None);
        };
        let e_polar = {
            let pts = pc.base().control_points()?;
            let psize = pts.iter().fold(0.0_f64, |m, c| {
                let w = if c.w == NOW { 1.0 } else { c.w };
                m.max((Vec3::new(c.x, c.y, c.z) * (1.0 / w)).length())
            });
            eval_bound(pts.iter().any(|c| c.w != NOW), pc.base().degree()?, psize)
        };
        let origin = rev.position.origin();
        let reach = net
            .iter()
            .flatten()
            .fold(0.0_f64, |m, x| m.max((*x - origin).length()));
        Ok(Some(Self {
            e_form,
            origin,
            reach,
            generator,
            polar: (
                pc.position.origin(),
                pc.position.x_axis(),
                pc.position.y_axis(),
                pc.radius_x,
                e_polar,
            ),
        }))
    }

    /// The bound at the STEP parameters `step` (degrees, and the generator's), the STEP
    /// point `s` and the form's point `f` ([`evaluates_as`] items 1 to 4).
    fn at(&self, step: Vec2, s: Vec3, f: Vec3) -> f64 {
        let u = crate::nurbs::math::UNIT;
        let rad = |deg: f64| deg * PI / 180.0;
        let dc = |deg: f64| 2.0 * u + gamma(3) * rad(deg).abs();
        let o = max_coord(self.origin);
        let size_f = f.length();
        // item 2 (per coordinate) and item 3 (as a length)
        let (coord, generator) = match self.generator {
            Generator::Torus { major, minor, c } => {
                let (m, h) = (major + minor, minor);
                let dm = gamma(3) * (major + minor) + 2.0 * minor * dc(step.y);
                let dh = gamma(1) * minor + 2.0 * minor * dc(step.y);
                (
                    gamma(3) * (o + m + h) + m * (gamma(2) + 2.0 * dc(step.x)) + dm + dh,
                    self.e_form + gamma(4) * (size_f + c) + gamma(4) * minor * rad(step.y).abs(),
                )
            }
            Generator::Sphere { radius, c } => {
                let dm = gamma(1) * radius + 2.0 * radius * dc(step.y);
                (
                    gamma(3) * (o + 2.0 * radius)
                        + radius * (gamma(2) + 2.0 * dc(step.x))
                        + 2.0 * dm,
                    self.e_form + gamma(4) * (size_f + c) + gamma(4) * radius * rad(step.y).abs(),
                )
            }
            Generator::Cone { r0, t, a, v0, v1 } => {
                let v = step.y;
                let m = r0.abs() + (v * t).abs();
                let dt = (1.0 + t * t) * gamma(3) * a.abs() + 2.0 * u * t.abs();
                let dm = gamma(2) * m + v.abs() * dt;
                (
                    gamma(3) * (o + m + v.abs()) + m * (gamma(2) + 2.0 * dc(step.x)) + dm,
                    gamma(6) * 2.0 * (v0.abs() + v1.abs()) * (1.0 + t.abs()),
                )
            }
            Generator::Spline { e_gen } => (
                gamma(8) * (o + 3.0 * self.reach) + 2.0 * self.reach * dc(step.x) + e_gen,
                0.0,
            ),
        };
        let e_step = 3.0_f64.sqrt() * coord;
        // item 4: the polar converter's inversion of its own circle's point
        let (c, x, y, rc, e_polar) = self.polar;
        let z = x.cross(y);
        let along = |v: Vec3| v - z * v.dot(z);
        let lever = along(s - c).length();
        let sweep = if rc > 0.0 {
            lever
                * ((e_polar + gamma(4) * (rc + 2.0 * c.length())) / rc
                    + gamma(4) * rad(step.x).abs()
                    + gamma(1) * 2.0 * PI)
        } else {
            // a circle of no radius has no angle: any, at the STEP point's lever
            lever * PI
        };
        self.e_form + e_step + generator + sweep
    }
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
