//! Whether a NURBS surface lies on an analytic surface: the analytic surface's implicit
//! equation composed with the NURBS in homogeneous coordinates is a polynomial in Bernstein
//! form on each Bézier patch, which vanishes identically exactly when the patch lies on the
//! surface. Its coefficients are checked against their rounding (#2388: a support kept as
//! its analytic kind beside its NURBS form, the two refused when they disagree).
//!
//! Not upstream; the fork's `SmBallPatch::IsOnSphere` makes the sphere's check this way.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::blend::fillet_section::interval::Ivl;
use crate::error::surface::SurfaceFailure;
use crate::error::{Failure, Result, SolidError, SM_ERR};
use crate::geom::axis2_placement::{axes_are_orthonormal, Axis2Placement};
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::{Vec2, Vec3, PI};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::{gamma, NOW, NOZ};
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
    /// How far `x` is from this surface: the whole surface, not a bounded patch of it
    /// (`offset_faces::on_target`).
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
    let nurb = form.gw_nurb().ok_or_else(|| {
        Failure::new(OP, SurfaceFailure::NoNurb)
            .with_status(SM_ERR)
            .into_error()
    })?;
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
    let nurb = curve
        .nurb
        .as_ref()
        .ok_or_else(|| SolidError::surface(OP, SurfaceFailure::CurveWithoutNurb))?;
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

/// The [`Residual`] of `a`'s equation over the Bézier patch of degrees `(p, q)` with
/// homogeneous points `points` (row by row).
fn bezier_residual<'a>(
    p: usize,
    q: usize,
    points: impl Iterator<Item = &'a crate::nurbs::math::CPoint>,
    a: &Implicit,
) -> Residual {
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
    let least_weight = ws.c.iter().copied().fold(f64::INFINITY, f64::min);
    Residual {
        coefficients: f.c,
        tol,
        degree,
        absolute,
        wmax,
        least_weight,
    }
}

/// Whether the Bézier patch of degrees `(p, q)` with homogeneous points `points` (row by
/// row) lies on `a` ([`lies_on`]): every coefficient of its [`Residual`] within the
/// rounding of computing it.
fn bezier_on<'a>(
    p: usize,
    q: usize,
    points: impl Iterator<Item = &'a crate::nurbs::math::CPoint>,
    a: &Implicit,
) -> bool {
    bezier_residual(p, q, points, a).within()
}

/// [`lies_on`]'s acceptance turned into a distance, ARGUED (not checked by a certificate):
/// how far a point of the surface `form` describes can lie from `a`'s surface, where
/// [`lies_on`] accepts `form`; `None` where it does not, for a torus, for a weight that is
/// not positive, for a patch whose rounding `τ` is below `MIN_POSITIVE / ε` or whose weight
/// power `W₀^d` is below `MIN_POSITIVE`, or for a bound (any patch's) that is not finite.
/// The `γ` model of `τ` leaves out underflow: each of a coefficient's `n` operations
/// (`n = 4d(p+1)(q+1) + 16`) may add up to `2⁻¹⁰⁷⁴` absolutely, `n · 2⁻¹⁰⁷⁴` in all. With
/// `τ ≥ MIN_POSITIVE / ε = 2⁻⁹⁷⁰` that is at most `n · 2⁻¹⁰⁴ · τ`, far inside the final
/// growth by `8ε` for any `n` below `2⁴⁹`. Points of tiny weight, whose coefficients
/// underflow to zeros that pass on nothing, are refused that way.
///
/// On a Bézier patch with least weight `W₀ > 0` (a rational patch's weight is a convex
/// combination of its points' weights, so at least `W₀` everywhere), every coefficient of
/// `a`'s equation of degree `d` in `(X W, W)` is computed within `τ` ([`lies_on`]'s
/// rounding) and accepted within `τ`, so the exact coefficient is within `2τ`, and by the
/// convex hull property of the Bernstein basis `|W^d F(X)| ≤ 2τ` over the whole patch:
/// `|F(X)| ≤ 2τ / W₀^d`. For a plane `F` is the signed distance (over the normal's
/// length, a few roundings from 1). For a cone whose stored axis `a` has `|a|² = 1 + δ`
/// (`δ` enclosed in `delta` by the caller; [`AnalyticSupport::implicit`] need not give a
/// unit axis), with `s` the height along the unit axis `â`, `ρ` the distance from it and
/// `R = r + t |a| s`: `|X|² − (a · X)² = ρ² − δ s²`, so `F = ρ² − δ s² − R²`. The surface is
/// one of revolution about `â`, of radius `ρ_P = √(R² + δ s²)` at height `s` where
/// `R² + δ s² ≥ 0`; there `ρ² − ρ_P² = F` and `|ρ − ρ_P| ≤ ρ + ρ_P`, so the point is within
/// `|ρ − ρ_P| ≤ √|F|` of the surface, radially at its height. With `δ < 0` the surface has
/// no point at a height where `|R| < √|δ| |s|` (a band about the apex), and a point there
/// has no such bound: a patch whose heights reach that band gives `None` (conservative;
/// with `δ ≥ 0`, or `δ` exactly 0 for a cardinal axis, every height has a point). A sphere
/// is the same with `|X|` for `ρ` and `R = r`. The square root is the bound near a cone's
/// apex, where `ρ_P` is small; elsewhere `|F| / (ρ + ρ_P)` is far smaller, but the bound
/// does not need it.
///
/// [`lies_on`] does not cover the rounding of the Bézier decomposition (knot insertion,
/// a chain of at most `p + q` convex combinations per point, each a few roundings of the
/// homogeneous terms' size): a patch point is moved by at most `γ · 2 absolute` in `H`
/// and `γ · W_max` in `W` (`γ = γ_{4(p+q)+4}`, `absolute` the largest coordinate of
/// `|H| + |O| W`), so the Euclidean point `X = H / W` by at most
/// `(|δH| + |X| |δW|) / W₀ ≤ 2 γ absolute (1 + W_max / W₀) / W₀`. That slack is added to
/// each patch's bound. The whole is the largest over the patches, grown by a few roundings.
///
/// # Errors
///
/// `SM_ERR` without NURB data; those of the Bézier decomposition.
pub fn distance_bound(form: &BSplineSurface, a: &Implicit, delta: [f64; 2]) -> Result<Option<f64>> {
    if matches!(a, Implicit::Torus { .. }) {
        return Ok(None);
    }
    // the most negative `δ` may be (0 when it cannot be below 0)
    let below = match delta {
        [lo, hi] if lo <= hi => (-lo).max(0.0),
        _ => return Ok(None),
    };
    let nurb = form.gw_nurb().ok_or_else(|| {
        Failure::new(OP, SurfaceFailure::NoNurb)
            .with_status(SM_ERR)
            .into_error()
    })?;
    let mut bound = 0.0_f64;
    for row in decompose_to_bez(nurb)? {
        for patch in row {
            let (p, q) = patch.degrees();
            let r = bezier_residual(p, q, patch.rows().flatten(), a);
            let w0 = r.least_weight;
            if !(r.within() && w0 > 0.0 && w0.is_finite()) {
                return Ok(None);
            }
            if let Implicit::Cone {
                origin,
                axis,
                radius,
                tan_angle,
            } = *a
            {
                if below > 0.0
                    && !clear_of_apex(
                        patch.rows().flatten(),
                        origin,
                        axis,
                        radius,
                        tan_angle,
                        below,
                    )
                {
                    return Ok(None);
                }
            }
            // `τ`'s rounding model leaves out underflow: with `τ ≥ MIN_POSITIVE / ε` the
            // coefficients' underflow (`n · 2⁻¹⁰⁷⁴`) is a relative `n · 2⁻¹⁰⁴` of `τ`, inside
            // the final growth; a smaller residual scale, or a weight power that underflows
            // (homogeneous points of tiny weight), has no bound
            let power = match r.degree {
                1 => w0,
                _ => w0 * w0,
            };
            if !(r.tol >= f64::MIN_POSITIVE / f64::EPSILON && power >= f64::MIN_POSITIVE) {
                return Ok(None);
            }
            let exact = 2.0 * r.tol;
            let residual = exact / power;
            let off = match *a {
                // the normal's length, a few roundings from 1
                Implicit::Plane { normal, .. } => {
                    residual / (normal.length() * (1.0 - 4.0 * f64::EPSILON))
                }
                _ => residual.sqrt(),
            };
            let g = gamma(4 * (p + q) + 4);
            // (`absolute` is a largest coordinate: a Euclidean length is under twice it)
            let slack = 2.0 * g * r.absolute * (1.0 + r.wmax / w0) / w0;
            // (`max` would drop a value that is not a number)
            let patch_bound = off + slack;
            if !patch_bound.is_finite() {
                return Ok(None);
            }
            bound = bound.max(patch_bound);
        }
    }
    let bound = bound * (1.0 + 8.0 * f64::EPSILON);
    Ok(bound.is_finite().then_some(bound))
}

/// Not upstream (tangency-labs/solidcraft#18166): a certified bound on how far every point
/// of the surface `form` lies from the plane or cone (a cylinder included) `a`, whether or
/// not it lies on it; `None` for a sphere or torus, a weight that is not positive, a cone
/// patch reaching the band about the apex where the bound does not hold, or a bound that is
/// not finite (not a number included).
///
/// PROVEN, per Bézier patch (as [`distance_bound`], whose terms these are): every Bernstein
/// coefficient of `a`'s equation of degree `d` in the patch's homogeneous coordinates is
/// computed within `τ` ([`lies_on`]'s rounding), so the exact one is within `τ` of the
/// computed, and by the convex hull property of the Bernstein basis
/// `|W^d F(X)| ≤ c + τ` over the whole patch, `c` the largest computed coefficient's size.
/// The weight is at least the least control weight `W₀ > 0` (a convex combination of
/// them), so `|F(X)| ≤ (c + τ) / W₀^d`. For a plane `F` is the signed distance times the
/// normal's length. For a cone, with `s` the height along the unit axis, `ρ` the distance
/// from it and `R = r + t |a| s` (`|a|² = 1 + δ`, `δ` enclosed from the stored axis),
/// `F = ρ² − ρ_P²` with `ρ_P = √(R² + δ s²)` the surface's radius at that height, and the
/// point of the surface at the same height and angle lies `|ρ − ρ_P| = |F| / (ρ + ρ_P) ≤
/// |F| / ρ_P` away. `ρ_P` is bounded below over the patch's heights by [`radius_floor`]
/// (`None` near the apex, where it is not).
///
/// PROVEN, the rounding of the extraction and decomposition (`γ_n = n u / (1 − n u)`,
/// `u = ε / 2` the unit roundoff, [`gamma`]): the patches are made from the net of
/// `source` (the surface `form` was cut from, or `form` itself) by knot insertions, each a
/// level of
/// [`CPoint::combine2`](crate::nurbs::math::CPoint::combine2) `α a + (1 − α) b` per
/// homogeneous coordinate, `0 ≤ α ≤ 1`:
/// - the ratio `α` takes 3 roundings, two subtractions and a division, so
///   `α̂ = α (1 + θ₃)`: `alf = (ul − left) / (knot − left)` in
///   [`extract_patch`](crate::nurbs::srf_tool::p1::extract_patch), and
///   `x = num / (u[isu + i] − u[isu])` with `num = u[ieu] − u[isu]` in
///   [`decompose_to_bez`];
/// - its complement `1 − α̂` takes 1 (`oma = 1.0 − alf`, `1.0 − x`);
/// - `combine2` takes 3, two products and a sum;
/// - so, against `α a + (1 − α) b` of the computed `a` and `b`, the `a` term carries
///   `α (1 + θ₅)` (ratio, product, sum) and the `b` term
///   `(1 − α) (1 + θ₃) − α θ₃ (1 + θ₃)` (complement, product, sum, and the ratio's error
///   through the complement); with `|a|, |b| ≤ M̂`, the largest computed size, the level's
///   new error is at most `(α (γ₅ + γ₃ + γ₃²) + (1 − α) γ₃) M̂ ≤ γ₈ M̂`
///   (`γ_j + γ_k + γ_j γ_k ≤ γ_{j+k}`), and the earlier errors pass through with the
///   exact factors `α` and `1 − α`, so a level's error is at most the larger of its two
///   inputs' plus `γ₈ M̂`.
///
/// `M` is the size of the original net: every intermediate point is within its error of a
/// convex combination of its control points, so each homogeneous coordinate is within
/// `max |H|∞` and each weight within `W_max` up to that error. After a chain of `L` levels
/// the error is at most `L γ₈ M̂ ≤ γ_{8L} M̂`, and with `M̂ ≤ M + γ_{8L} M̂` at most
/// `γ_{8L} / (1 − γ_{8L}) M ≤ γ_{8L+1} M` (as `8L (8L + 1) u ≤ 1`).
///
/// `L`, the longest chain of levels behind one point of a patch, is at most
/// `L_u + L_v` with `L_u = 2p + p (p − 1) / 2` and `L_v` alike in `q`
/// (`insertion_levels`), counted from the loops that run (`nurbs/srf_tool/p1.rs`):
/// - the extraction in u ([`extract_patch`](crate::nurbs::srf_tool::p1::extract_patch),
///   per column): the left knot's round `i` (`1..=p − mul`) sets `aw[j]` from `aw[j]` and
///   `aw[j + 1]` of round `i − 1`, one level per round, so at most `p − mul`; the right
///   knot's round `i` (`1..=p − mur`) sets `aw[k]` from `aw[k]` and `aw[k − 1]`, which may
///   be left-inserted points, so at most `(p − mul) + (p − mur) ≤ 2p`; then in v, per
///   row of those points, `(q − mvl) + (q − mvr)` more;
/// - the decomposition in u ([`decompose_to_bez`], per column): at a knot of multiplicity
///   `μ < p` it runs `r = p − μ` rounds; round `i` sets `bw[j]` for `j = p` down to `μ + i`
///   from `bw[j]` and `bw[j − 1]`, and saves `bw[p]` as the next strip's point `r − i`
///   (`nbw[save]`), whose points `r..=p` come fresh from the net. So a chain runs on
///   through the saves of earlier knots: at the start of every strip the point at
///   position `x` is at most `B(x) = (p − x) (p − x − 1) / 2` levels past the net it is
///   decomposed from. Fresh points are 0; a saved point `r − i` is `bw[p]` after `i`
///   rounds, at most `i + B(p − i) = i (i + 1) / 2 ≤ B(r − i) = (μ + i) (μ + i − 1) / 2`
///   (`μ ≥ 1`, `B` falls with `x`). The strip's point `j`, set in `n` rounds
///   (`j − n ≥ μ`, so `k = p − (j − n) ≤ p − 1` and `n ≤ k`), is at most
///   `n + B(j − n) ≤ k (k + 1) / 2 ≤ p (p − 1) / 2`, and an untouched one at most
///   `B(0) = p (p − 1) / 2`. Simple knots reach it: the chain runs back through the saves
///   of `p − 1` earlier knots, `Σ_{i=1}^{p−1} i`;
/// - the decomposition in v, per row of each strip, the same on the strip's points:
///   `q (q − 1) / 2` more.
///
/// `γ_{8L+16}` covers `γ_{8L+1}` and `8` more roundings for the coefficients' scale. The
/// test `the_rounding_count_covers_the_insertion_chains` runs the loops above on tracked
/// points for degrees 1 to 10 (bit-equal to the real extraction and decomposition) and
/// checks the count against the deepest chain they make. In the coordinates moved to the equation's origin
/// `O` the error is at most `γ A` with `A = max |H|∞ + |O|∞ W_max`, and in the weight at
/// most `γ W_max`, so at a point with weight `W ≥ W₀`, `X − O = (H − O W) / W` moves by at
/// most `γ A / W₀ + A (1 + γ) γ W_max / (W₀ (W₀ − γ W_max))`, twice that as a Euclidean
/// length (`√3 < 2`); `None` where `W₀ ≤ γ W_max`. The net's size is the larger of
/// `source`'s and `form`'s and the patch's own (so `form` alone, as `source`, is sound when
/// it was not cut from another net). The margin against the uses is wide: the slack is a
/// relative `2 γ_{160} ≈ 3.6e-14` for a bicubic patch (`L = 18`), against bounds compared with
/// tolerances of `1e-6` and more. A patch whose rounding `τ` is below `MIN_POSITIVE / ε`
/// (where the coefficients' underflow is not inside the `γ` model, as [`distance_bound`])
/// has no bound. The whole is the largest over the patches, grown by a few roundings.
///
/// # Errors
///
/// `SM_ERR` without NURB data; those of the Bézier decomposition.
pub(crate) fn off_surface_bound(
    form: &BSplineSurface,
    a: &Implicit,
    source: &BSplineSurface,
) -> Result<Option<f64>> {
    let below = match *a {
        Implicit::Plane { .. } => 0.0,
        Implicit::Cone { axis, .. } => {
            let sq = |x: f64| Ivl::from(x) * Ivl::from(x);
            let delta = sq(axis.x) + sq(axis.y) + sq(axis.z) - Ivl::from(1.0);
            if !delta.is_finite() {
                return Ok(None);
            }
            (-delta.lo).max(0.0)
        }
        Implicit::Sphere { .. } | Implicit::Torus { .. } => return Ok(None),
    };
    let nurb = form.gw_nurb().ok_or_else(|| {
        Failure::new(OP, SurfaceFailure::NoNurb)
            .with_status(SM_ERR)
            .into_error()
    })?;
    let origin = match *a {
        Implicit::Plane { origin, .. } | Implicit::Cone { origin, .. } => origin,
        Implicit::Sphere { centre, .. } | Implicit::Torus { centre, .. } => centre,
    };
    let Some(net) = source.gw_nurb() else {
        return Ok(None);
    };
    let (Some(from), Some(own)) = (net_size(net, origin), net_size(nurb, origin)) else {
        return Ok(None);
    };
    let (net_absolute, net_wmax) = (from.0.max(own.0), from.1.max(own.1));
    let mut bound = 0.0_f64;
    for row in decompose_to_bez(nurb)? {
        for patch in row {
            let (p, q) = patch.degrees();
            let r = bezier_residual(p, q, patch.rows().flatten(), a);
            let w0 = r.least_weight;
            if !(w0 > 0.0 && w0.is_finite()) {
                return Ok(None);
            }
            let mut top = 0.0_f64;
            for c in &r.coefficients {
                // (not a number refuses: `max` would drop it)
                if !c.is_finite() {
                    return Ok(None);
                }
                top = top.max(c.abs());
            }
            let power = match r.degree {
                1 => Ivl::from(w0),
                _ => Ivl::from(w0) * Ivl::from(w0),
            };
            // (not a number refuses; as [`distance_bound`], the `γ` model leaves out
            // underflow, inside the final growth only with `τ ≥ MIN_POSITIVE / ε`)
            let normal = power.lo >= f64::MIN_POSITIVE && r.tol >= f64::MIN_POSITIVE / f64::EPSILON;
            if !normal {
                return Ok(None);
            }
            let f = (Ivl::from(top) + Ivl::from(r.tol)) / power;
            let off = match *a {
                // the normal's length, a few roundings from its computed value
                Implicit::Plane { normal, .. } => {
                    (f / Ivl::from(normal.length() * (1.0 - 4.0 * f64::EPSILON))).hi
                }
                Implicit::Cone {
                    origin,
                    axis,
                    radius,
                    tan_angle,
                } => {
                    let Some(floor) = radius_floor(
                        patch.rows().flatten(),
                        origin,
                        axis,
                        radius,
                        tan_angle,
                        below,
                    ) else {
                        return Ok(None);
                    };
                    (f / Ivl::from(floor)).hi
                }
                Implicit::Sphere { .. } | Implicit::Torus { .. } => return Ok(None),
            };
            let g = gamma(rounding_index(p, q));
            let absolute = net_absolute.max(r.absolute);
            let wmax = net_wmax.max(r.wmax);
            // (the true weight is at least `W₀ − γ W_max`)
            let low = w0 - g * wmax;
            if !(low > 0.0 && low.is_finite()) {
                return Ok(None);
            }
            // (`absolute` is a largest coordinate: a Euclidean length is under twice it)
            let slack = 2.0 * g * absolute * (1.0 / w0 + (1.0 + g) * wmax / (w0 * low));
            let patch_bound = off + slack;
            if !patch_bound.is_finite() {
                return Ok(None);
            }
            bound = bound.max(patch_bound);
        }
    }
    let bound = bound * (1.0 + 8.0 * f64::EPSILON);
    Ok(bound.is_finite().then_some(bound))
}

/// A complete surface of revolution as the certified line pass's side test reads it
/// (`surface::surface::certified_line`, module docs "Typing"): its side function `f` is
/// defined from the placement's stored axes `x`, `y`, `z` and origin `O` as they are, with
/// `w = X − O`, `ρ = √((x · w)² + (y · w)²)` and `h = z · w`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Revolution {
    /// `f = |w| − radius`, from `F = |w|² − radius²` (the stored axes are not read).
    Sphere {
        /// The centre `O`.
        centre: Vec3,
        /// The radius, positive.
        radius: f64,
    },
    /// `f = (ρ − R) / √(1 + slope²)` with `R = radius + slope h`, from
    /// `F = ρ² − R² = (ρ − R)(ρ + R)` (a cylinder for `slope = 0`).
    Cone {
        /// `O`.
        origin: Vec3,
        /// The stored axes `x`, `y`, `z`.
        frame: [Vec3; 3],
        /// The radius at `h = 0`.
        radius: f64,
        /// The radius's change per unit `h`.
        slope: f64,
    },
    /// `f = d − minor` with `d = √((ρ − major)² + h²)`, a ring torus (`major > minor > 0`),
    /// from `F = (ρ² + h² + major² − minor²)² − 4 major² ρ²
    /// = (d − minor)(d + minor)((ρ + major)² + h² − minor²)`.
    Torus {
        /// `O`.
        origin: Vec3,
        /// The stored axes `x`, `y`, `z`.
        frame: [Vec3; 3],
        /// The major radius.
        major: f64,
        /// The minor radius.
        minor: f64,
    },
}

/// What [`side_bounds`] proves of one Bézier patch of a net.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SideBound {
    /// The patch's parameter rectangle, `[[u0, u1], [v0, v1]]`.
    pub span: [[f64; 2]; 2],
    /// A certified bound on `|F|` ([`Revolution`]) at every point of the net over the
    /// patch; `None` where none is shown (a weight not proved positive, a bound that is not
    /// finite).
    pub residual: Option<f64>,
}

/// A polynomial in Bernstein form over a patch with interval coefficients, degrees
/// `(p, q)`, `c[i (q + 1) + j]`: each coefficient encloses the exact one of every
/// polynomial whose inputs lie in the inputs' intervals.
#[derive(Debug, Clone, PartialEq)]
struct IPoly {
    p: usize,
    q: usize,
    c: Vec<Ivl>,
}

/// `C(n, k)` exactly, as a double (exact for every `n` the degrees here reach: the
/// products are integers below `2⁵³`); `None` past that.
fn exact_binom(n: usize, k: usize) -> Option<f64> {
    let mut r: u64 = 1;
    for i in 0..k {
        // (r C(n, i) times (n − i) is divisible by i + 1)
        let n_i = u64::try_from(n.checked_sub(i)?).ok()?;
        let d = u64::try_from(i + 1).ok()?;
        r = r.checked_mul(n_i)?.checked_div(d)?;
    }
    #[allow(clippy::cast_precision_loss)]
    let f = r as f64;
    (r < (1_u64 << 53)).then_some(f)
}

impl IPoly {
    fn at(&self, i: usize, j: usize) -> Ivl {
        self.c
            .get(i * (self.q + 1) + j)
            .copied()
            .unwrap_or_else(Ivl::whole)
    }

    /// The product, of degrees `(p + p', q + q')`; `None` past the exact binomials.
    fn mul(&self, o: &Self) -> Option<Self> {
        let (p, q) = (self.p + o.p, self.q + o.q);
        let mut c = vec![Ivl::from(0.0); (p + 1) * (q + 1)];
        for i in 0..=self.p {
            for j in 0..=self.q {
                for k in 0..=o.p {
                    for l in 0..=o.q {
                        let num = Ivl::from(exact_binom(self.p, i)?)
                            * Ivl::from(exact_binom(o.p, k)?)
                            * Ivl::from(exact_binom(self.q, j)?)
                            * Ivl::from(exact_binom(o.q, l)?);
                        let den =
                            Ivl::from(exact_binom(p, i + k)?) * Ivl::from(exact_binom(q, j + l)?);
                        let slot = c.get_mut((i + k) * (q + 1) + j + l)?;
                        *slot = *slot + num / den * self.at(i, j) * o.at(k, l);
                    }
                }
            }
        }
        Some(Self { p, q, c })
    }

    /// `a self + b o`, of the same degrees.
    fn combine(&self, a: Ivl, o: &Self, b: Ivl) -> Self {
        Self {
            p: self.p,
            q: self.q,
            c: self
                .c
                .iter()
                .zip(&o.c)
                .map(|(&x, &y)| a * x + b * y)
                .collect(),
        }
    }

    /// The largest magnitude of a coefficient, rounded up (not a number: the whole line's).
    fn mag(&self) -> f64 {
        self.c.iter().fold(0.0_f64, |m, x| {
            if x.is_finite() {
                m.max(x.mag())
            } else {
                f64::INFINITY
            }
        })
    }
}

/// Not upstream (#20499): for each Bézier patch of the net `net`, a certified bound on the
/// implicit polynomial `F` of `rev` ([`Revolution`]) over the net's points there, which the
/// certified line pass's side test divides by `F`'s other factor to bound its side function
/// (`surface::surface::certified_line`, module docs "Typing"), in place of a check at
/// sample points.
///
/// PROVEN, per patch. The patches are made from the net by knot insertions, so each
/// computed homogeneous point, moved to `O`, is within `γ A` of the exact one in every
/// coordinate and its weight within `γ W_max` (`γ = γ_(8L+16)`, `A = max |H|∞ + |O|∞ W_max`
/// of the net and the patch: [`off_surface_bound`]'s derivation, §4.4). Each point is
/// enclosed in intervals that widen it by those, moved to `O` and, for the cone and the
/// torus, into the frame (`q = M (H − O W)`, `M` the rows `x`, `y`, `z`) in rounded interval
/// arithmetic; the exact patch's points then lie in the boxes. `W^d F` (`d` = 2 for the
/// sphere and the cone, 4 for the torus) is composed from them in Bernstein form with
/// interval coefficients (each product's binomial weights exact integers), so each encloses
/// the exact patch's coefficient, and by the convex hull property of the Bernstein basis
/// `|W^d F| ≤ c` over the patch, `c` the largest coefficient magnitude. The exact weight is a
/// convex combination of the exact control weights, so at least their least lower end
/// `W₀ > 0`: `|F| ≤ c / W₀^d`. Nothing is sampled.
///
/// # Errors
///
/// Those of the Bézier decomposition.
pub(crate) fn side_bounds(
    net: &crate::nurbs::srf_basic::Surface,
    rev: &Revolution,
) -> Result<Vec<SideBound>> {
    let (origin, frame) = match *rev {
        Revolution::Sphere { centre, .. } => (centre, None),
        Revolution::Cone { origin, frame, .. } | Revolution::Torus { origin, frame, .. } => {
            (origin, Some(frame))
        }
    };
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    let frame_ok = frame.is_none_or(|f| f.iter().all(|&a| finite(a)));
    let Some(net_size) = net_size(net, origin).filter(|_| finite(origin) && frame_ok) else {
        // (no patch with a bound: a component reads none as no bound)
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for row in decompose_to_bez(net)? {
        for patch in row {
            let (p, q) = patch.degrees();
            let span = |knots: &[f64], d: usize| match (knots.get(d), knots.get(d + 1)) {
                (Some(&a), Some(&b)) => Some([a, b]),
                _ => None,
            };
            let (Some(su), Some(sv)) = (span(patch.u_knots(), p), span(patch.v_knots(), q)) else {
                // (a patch left out would leave its points unbounded: no patch has a bound)
                return Ok(Vec::new());
            };
            let residual = patch_residual(&patch, rev, (origin, frame), net_size);
            out.push(SideBound {
                span: [su, sv],
                residual,
            });
        }
    }
    Ok(out)
}

/// [`side_bounds`] on one Bézier patch: `None` where it shows none.
fn patch_residual(
    patch: &crate::nurbs::srf_basic::Surface,
    rev: &Revolution,
    (origin, frame): (Vec3, Option<[Vec3; 3]>),
    (net_absolute, net_wmax): (f64, f64),
) -> Option<f64> {
    let (p, q) = patch.degrees();
    let own = net_size(patch, origin)?;
    let g = gamma(rounding_index(p, q));
    let absolute = net_absolute.max(own.0);
    let wmax = net_wmax.max(own.1);
    let (spread, wspread) = (
        Ivl::new(-g * absolute, g * absolute),
        Ivl::new(-g * wmax, g * wmax),
    );
    if !(spread.is_finite() && wspread.is_finite()) {
        return None;
    }
    let o = [origin.x, origin.y, origin.z].map(Ivl::from);
    // the exact patch's points, enclosed: moved to `O` (and into the frame), and weights
    let mut coords: [Vec<Ivl>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut weights: Vec<Ivl> = Vec::new();
    for c in patch.rows().flatten() {
        let wi = if c.w == NOW { 1.0 } else { c.w };
        let cz = if c.z == NOZ { 0.0 } else { c.z };
        let w = Ivl::from(wi) + wspread;
        let [o0, o1, o2] = o;
        let moved = [
            Ivl::from(c.x) - o0 * Ivl::from(wi) + spread,
            Ivl::from(c.y) - o1 * Ivl::from(wi) + spread,
            Ivl::from(cz) - o2 * Ivl::from(wi) + spread,
        ];
        let x = match frame {
            None => moved,
            Some(rows) => rows.map(|a| {
                let [m0, m1, m2] = moved;
                Ivl::from(a.x) * m0 + Ivl::from(a.y) * m1 + Ivl::from(a.z) * m2
            }),
        };
        for (slot, xi) in coords.iter_mut().zip(x) {
            slot.push(xi);
        }
        weights.push(w);
    }
    let w0 = weights.iter().fold(f64::INFINITY, |m, w| m.min(w.lo));
    if !(w0 > 0.0 && w0.is_finite()) {
        return None;
    }
    let poly = |c: Vec<Ivl>| IPoly { p, q, c };
    let [x0, x1, x2] = coords;
    let (a, b, h, w) = (poly(x0), poly(x1), poly(x2), poly(weights));
    let one = Ivl::from(1.0);
    let sq = |x: &IPoly| x.mul(x);
    let (f, degree) = match *rev {
        Revolution::Sphere { radius, .. } => {
            let r2 = Ivl::from(radius) * Ivl::from(radius);
            let n2 = sq(&a)?
                .combine(one, &sq(&b)?, one)
                .combine(one, &sq(&h)?, one);
            (n2.combine(one, &sq(&w)?, Ivl::from(0.0) - r2), 2)
        }
        Revolution::Cone { radius, slope, .. } => {
            let r = w.combine(Ivl::from(radius), &h, Ivl::from(slope));
            let rho2 = sq(&a)?.combine(one, &sq(&b)?, one);
            (rho2.combine(one, &sq(&r)?, Ivl::from(-1.0)), 2)
        }
        Revolution::Torus { major, minor, .. } => {
            let (big, small) = (Ivl::from(major), Ivl::from(minor));
            let rho2 = sq(&a)?.combine(one, &sq(&b)?, one);
            let w2 = sq(&w)?;
            let n2 = rho2.combine(one, &sq(&h)?, one);
            let gw = n2.combine(one, &w2, big * big - small * small);
            let four = Ivl::from(4.0) * big * big;
            (
                sq(&gw)?.combine(one, &rho2.mul(&w2)?, Ivl::from(0.0) - four),
                4,
            )
        }
    };
    let top = f.mag();
    let w0i = Ivl::new(w0, w0);
    let power = if degree == 2 {
        w0i * w0i
    } else {
        let w2 = w0i * w0i;
        w2 * w2
    };
    if power.lo.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return None;
    }
    let bound = (Ivl::from(top) / Ivl::new(power.lo, power.lo)).hi;
    bound.is_finite().then_some(bound)
}

/// The longest chain of knot-insertion levels behind a point of [`off_surface_bound`]'s
/// Bézier patches of degrees `(p, q)`, `L_u + L_v` with `L_u = 2p + p (p − 1) / 2` (the
/// extraction's two ends and the decomposition's saved points; derived there) and `L_v`
/// alike in `q`. Saturates (a degree that large has no net to decompose).
fn insertion_levels(p: usize, q: usize) -> usize {
    let per = |d: usize| {
        d.saturating_mul(2)
            .saturating_add(d.saturating_mul(d.saturating_sub(1)) / 2)
    };
    per(p).saturating_add(per(q))
}

/// The `n` of [`off_surface_bound`]'s `γ_n`: `8 L + 16`, `L` the [`insertion_levels`]
/// (eight roundings a level, one for the size's feedback, eight for the coefficients'
/// scale, the rest spare).
fn rounding_index(p: usize, q: usize) -> usize {
    insertion_levels(p, q).saturating_mul(8).saturating_add(16)
}

/// The size of the control net `net` for [`off_surface_bound`]'s rounding, moved to the
/// origin `origin`: `(max |H|∞ + |O|∞ W_max, W_max)` over its homogeneous points `H`
/// (stored weighted when rational) and weights `W` (1 when not rational); `None` when a
/// value is not finite.
fn net_size(net: &crate::nurbs::srf_basic::Surface, origin: Vec3) -> Option<(f64, f64)> {
    let mut hmax = 0.0_f64;
    let mut wmax = 0.0_f64;
    for c in net.rows().flatten() {
        let wi = if c.w == NOW { 1.0 } else { c.w };
        let z = if c.z == NOZ { 0.0 } else { c.z };
        // (each coordinate checked: `max` would drop a value that is not a number)
        if !(c.x.is_finite() && c.y.is_finite() && z.is_finite() && wi.is_finite()) {
            return None;
        }
        let hm = c.x.abs().max(c.y.abs()).max(z.abs());
        hmax = hmax.max(hm);
        wmax = wmax.max(wi.abs());
    }
    let om = origin.x.abs().max(origin.y.abs()).max(origin.z.abs());
    let absolute = (hmax + om * wmax) * (1.0 + 2.0 * f64::EPSILON);
    absolute.is_finite().then_some((absolute, wmax))
}

/// A lower bound of the cone's radius `ρ_P = √(R² + δ s²)` over the heights `s` of the
/// Bézier patch with homogeneous points `points` ([`off_surface_bound`]), `δ ≥ −below`:
/// `ρ_P ≥ √(R_min² − below s_max²)`, `R_min` the least `|R|` over the heights (`R` linear
/// in `s`, of one sign over them, so reached at an end) and `s_max` the largest `|s|`. The
/// heights lie between the Euclidean points' (the patch is in their convex hull, `s`
/// linear), widened by their rounding as [`clear_of_apex`]'s. `None` where `R` changes
/// sign over them, the floor is not positive, or a value is not a number.
fn radius_floor<'a>(
    points: impl Iterator<Item = &'a crate::nurbs::math::CPoint>,
    origin: Vec3,
    axis: Vec3,
    radius: f64,
    tan_angle: f64,
    below: f64,
) -> Option<f64> {
    let length = axis.length();
    if !(length > 0.0 && length.is_finite()) {
        return None;
    }
    let unit = axis * (1.0 / length);
    let (mut lo, mut hi, mut size) = (f64::INFINITY, f64::NEG_INFINITY, 0.0_f64);
    for c in points {
        let w = if c.w == NOW { 1.0 } else { c.w };
        // (not a number is not positive)
        if w.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
            return None;
        }
        let x = Vec3::new(c.x, c.y, c.z) * (1.0 / w) - origin;
        let s = x.dot(unit);
        lo = lo.min(s);
        hi = hi.max(s);
        size = size.max(x.x.abs().max(x.y.abs()).max(x.z.abs()));
    }
    let spread =
        32.0 * f64::EPSILON * (size + origin.x.abs().max(origin.y.abs()).max(origin.z.abs()));
    let (lo, hi) = (lo - spread, hi + spread);
    if !(lo <= hi && spread.is_finite()) {
        return None;
    }
    // `R = r + t |a| s` at the ends, enclosed (the unit axis's length rounds within a few ε)
    let slope = Ivl::from(tan_angle)
        * Ivl::new(
            length * (1.0 - 4.0 * f64::EPSILON),
            length * (1.0 + 4.0 * f64::EPSILON),
        );
    let r_at = |s: f64| Ivl::from(radius) + slope * Ivl::from(s);
    let (r_lo, r_hi) = (r_at(lo), r_at(hi));
    let r_min = if r_lo.lo > 0.0 && r_hi.lo > 0.0 {
        r_lo.lo.min(r_hi.lo)
    } else if r_lo.hi < 0.0 && r_hi.hi < 0.0 {
        (-r_lo.hi).min(-r_hi.hi)
    } else {
        return None;
    };
    let s_max = lo.abs().max(hi.abs());
    let floor2 = Ivl::from(r_min) * Ivl::from(r_min)
        - Ivl::from(below) * Ivl::from(s_max) * Ivl::from(s_max);
    let floor = floor2.sqrt().lo;
    (floor > 0.0 && floor.is_finite()).then_some(floor)
}

/// Whether every height `s` of the Bézier patch with homogeneous points `points` along
/// the cone's unit axis keeps `|R(s)| > √below |s|` (`R = r + t |a| s`): the cone with
/// `|a|² = 1 + δ`, `δ ≥ −below`, has a point at each of them ([`distance_bound`]). The
/// heights lie between the Euclidean points' (the patch is in their convex hull, `s`
/// linear), widened by their rounding; `R` keeps one sign over them, so `|R(s)| − √below |s|`
/// is linear between the ends and 0 and is checked there. Not a number fails.
fn clear_of_apex<'a>(
    points: impl Iterator<Item = &'a crate::nurbs::math::CPoint>,
    origin: Vec3,
    axis: Vec3,
    radius: f64,
    tan_angle: f64,
    below: f64,
) -> bool {
    let length = axis.length();
    if !(length > 0.0 && length.is_finite()) {
        return false;
    }
    let unit = axis * (1.0 / length);
    let (mut lo, mut hi, mut size) = (f64::INFINITY, f64::NEG_INFINITY, 0.0_f64);
    for c in points {
        let w = if c.w == NOW { 1.0 } else { c.w };
        // (not a number is not positive)
        if w.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
            return false;
        }
        let x = Vec3::new(c.x, c.y, c.z) * (1.0 / w) - origin;
        let s = x.dot(unit);
        lo = lo.min(s);
        hi = hi.max(s);
        size = size.max(x.x.abs().max(x.y.abs()).max(x.z.abs()));
    }
    // each height's rounding: the division, the move to the origin, the unit axis and the
    // dot product, a few roundings of the coordinates' size each
    let spread =
        32.0 * f64::EPSILON * (size + origin.x.abs().max(origin.y.abs()).max(origin.z.abs()));
    let (lo, hi) = (lo - spread, hi + spread);
    if !(lo <= hi && spread.is_finite()) {
        return false;
    }
    let slope = tan_angle * length;
    let r_of = |s: f64| radius + slope * s;
    // `R`'s own rounding at a height
    let r_err = |s: f64| 4.0 * f64::EPSILON * (radius.abs() + (slope * s).abs());
    let (r_lo, r_hi) = (r_of(lo), r_of(hi));
    let same_sign = (r_lo - r_err(lo) > 0.0 && r_hi - r_err(hi) > 0.0)
        || (r_lo + r_err(lo) < 0.0 && r_hi + r_err(hi) < 0.0);
    if !same_sign {
        return false;
    }
    let root = below.sqrt() * (1.0 + 4.0 * f64::EPSILON);
    let clear = |s: f64| r_of(s).abs() - r_err(s) > root * s.abs();
    clear(lo) && clear(hi) && (!(lo < 0.0 && hi > 0.0) || clear(0.0))
}

/// `a`'s equation over a Bézier patch in its homogeneous coordinates ([`lies_on`]).
struct Residual {
    /// Its Bernstein coefficients.
    coefficients: Vec<f64>,
    /// The rounding each is computed within.
    tol: f64,
    /// The equation's degree.
    degree: usize,
    /// The largest absolute homogeneous term, `|H| + |O| W`.
    absolute: f64,
    /// The largest weight.
    wmax: f64,
    /// The least weight.
    least_weight: f64,
}

impl Residual {
    /// Every coefficient within its rounding (not a number is not).
    fn within(&self) -> bool {
        self.coefficients.iter().all(|v| {
            matches!(
                v.abs().partial_cmp(&self.tol),
                Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
            )
        })
    }
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

/// A Bezier segment of a rational curve, Bernstein polynomials of degree `(p, 0)`: the
/// homogeneous `(Y - O V)` components and the weight `V`.
type CurveSegment = ([BPoly; 3], BPoly);

/// A rational curve's Bezier segments ([`CurveSegment`]), `O` the curve's first point.
fn curve_segments(curve: &BSplineCurve) -> Result<(Vec<CurveSegment>, Vec3)> {
    let nurb = curve
        .nurb
        .as_ref()
        .ok_or_else(|| SolidError::surface(OP_EXACT, SurfaceFailure::CurveWithoutNurb))?;
    let w_of = |w: f64| if w == NOW { 1.0 } else { w };
    let origin = nurb
        .cpts()
        .first()
        .map(|c| Vec3::new(c.x, c.y, c.z) * (1.0 / w_of(c.w)))
        .ok_or_else(|| SolidError::surface(OP_EXACT, SurfaceFailure::CurveWithoutControlPoints))?;
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
    let nurb = form.gw_nurb().ok_or_else(|| {
        Failure::new(OP_EXACT, SurfaceFailure::NoNurb)
            .with_status(SM_ERR)
            .into_error()
    })?;
    let (segs, o) = curve_segments(spine)?;
    // `curve_segments` above has returned `CurveWithoutNurb` for a spine without its NURB
    let spine_breaks = spine
        .nurb
        .as_ref()
        .map_or_else(Vec::new, |n| breaks(n.knots()));
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
    #[must_use]
    pub fn implicit(&self) -> Implicit {
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
    /// as given; a cylinder, cone or sphere is parametrised as the kernel recognises it in
    /// `form` ([`recognise`]: the sweep angles, generator range, swap and inside-out are the
    /// form's) and must be of this support's kind; a torus is parametrised as `form` is
    /// about this support's own axis ([`Torus::of_support`], which fits nothing, so a tube
    /// that shows a short arc is carried as any other). Each carries this support's own
    /// placement, radii and semi-angle on that parametrisation, so that
    /// [`AnalyticSupport::of`] reads the support back unchanged. It must evaluate as `form`
    /// at the form's own parameters ([`evaluates_as`]), which together with [`lies_on`] is
    /// what proves the support and the form agree: [`lies_on`] proves the point set, and
    /// the evaluation proves the parametrisation, including the frame the placement imposes
    /// about the axis and, for a sphere, the whole orientation, none of which the implicit
    /// equation constrains (#2588). The surface takes `form` as its NURB. A short arc fails
    /// the evaluation: a centre rebuilt from it is good only to about the radius over the
    /// square of the sweep. [`Self::place_on`] carries a cone or cylinder whose own axis the
    /// form lies on.
    ///
    /// # Errors
    ///
    /// `InvalidInput` where `form` is off the analytic surface or is not its
    /// parametrisation; those of the recognisers, the evaluations and [`lies_on`].
    pub fn build(&self, k: &Kernel, form: BSplineSurface) -> Result<Surface> {
        if !self.placement_is_square() {
            return Err(SolidError::invalid_input(
                OP_BUILD,
                "a support's placement is not orthonormal",
            ));
        }
        let implicit_ok = lies_on(&form, &self.implicit())?;
        if let Self::Plane {
            placement,
            uv_scale,
            domain,
        } = *self
        {
            if !implicit_ok {
                return Err(SolidError::invalid_input(
                    OP_BUILD,
                    "a support's analytic kind disagrees with its NURBS form",
                ));
            }
            return Ok(Surface::from(Plane::from_stored(
                placement, uv_scale, domain, form,
            )));
        }
        // a surface of revolution: the one the kernel recognises in the form (a torus: the one
        // the form is about the support's axis), which must be this one, take the form as its
        // NURB and evaluate as the form at the form's own parameters
        let surface = self.on_form(k, &form)?;
        if implicit_ok && evaluates_as(k, &surface, &form)? {
            return Ok(surface);
        }
        if !implicit_ok {
            return Err(SolidError::invalid_input(
                OP_BUILD,
                "a support's analytic kind disagrees with its NURBS form",
            ));
        }
        Err(not_form())
    }

    /// This cone or cylinder on `form`: the form's sweep and generator, and this support's
    /// axis, radius and semi-angle. The form lies on this support. A short sweep, whose
    /// trigonometric point is a few ulps off its Bézier, keeps these numbers.
    ///
    /// # Errors
    ///
    /// `InvalidInput` where the placement is not orthonormal, `form` is off this support,
    /// or the kernel recognises no cone in `form`; those of the recogniser and [`lies_on`].
    pub(crate) fn place_on(&self, k: &Kernel, form: &BSplineSurface) -> Result<Surface> {
        if !matches!(self, Self::Cylinder { .. } | Self::Cone { .. }) {
            return Err(not_form());
        }
        if !self.placement_is_square() {
            return Err(SolidError::invalid_input(
                OP_BUILD,
                "a support's placement is not orthonormal",
            ));
        }
        if !lies_on(form, &self.implicit())? {
            return Err(SolidError::invalid_input(
                OP_BUILD,
                "a support's analytic kind disagrees with its NURBS form",
            ));
        }
        self.on_form(k, form)
    }

    /// The placement's axes as stored: unit and square to rounding (8 ulps).
    fn placement_is_square(&self) -> bool {
        let p = match self {
            Self::Plane { placement, .. }
            | Self::Cylinder { placement, .. }
            | Self::Cone { placement, .. }
            | Self::Sphere { placement, .. }
            | Self::Torus { placement, .. } => placement,
        };
        axes_are_orthonormal(p.x_axis(), p.y_axis())
    }

    /// The largest ratio, over the form's Greville points, of this support's analytic
    /// surface's distance from `form` to the rounding [`evaluates_as`] derives there (#3185):
    /// at most 1 where [`Self::build`]'s parametrisation test passes. The surface is the one
    /// `build` makes (`form`'s parametrisation, carrying this support's numbers);
    /// `lies_on` and the placement's axes are not checked here. `None` for a plane, or a
    /// surface the bound does not cover.
    ///
    /// # Errors
    ///
    /// `InvalidInput` where the kernel recognises no surface of this kind in `form`, or
    /// `form` gives a torus no reading about this support's axis; those of the recognisers,
    /// the conversions and the evaluations.
    pub fn parametrisation_margin(&self, k: &Kernel, form: &BSplineSurface) -> Result<Option<f64>> {
        if matches!(self, Self::Plane { .. }) {
            return Ok(None);
        }
        let surface = self.on_form(k, form)?;
        evaluation_margin(k, &surface, form)
    }

    /// The surface of revolution of this support's kind on `form`'s parametrisation, carrying
    /// this support's own numbers and `form` as its NURB ([`Self::build`] after its checks of
    /// the point set, before the parametrisation's). A cylinder, cone or sphere takes the
    /// parametrisation the kernel recognises in `form`; a torus's is read off `form` about
    /// this support's own axis ([`Torus::of_support`]).
    fn on_form(&self, k: &Kernel, form: &BSplineSurface) -> Result<Surface> {
        let form = form.copy();
        // a torus's support says where its axis and its tube are, so its conventions are read
        // off the form about that axis and nothing is fitted. Recognition fits a circle to
        // each meridian, which a tube that shows a short arc does not fix: it finds no torus
        // there, and would leave a support that is right to the last bit nothing to stand on.
        if let Self::Torus {
            placement,
            major_radius,
            minor_radius,
        } = *self
        {
            return Torus::of_support(k, placement, major_radius, minor_radius, &form)?
                .map(Surface::from)
                .ok_or_else(not_form);
        }
        let mut surface = recognise(k, self.kind(), &form)?.ok_or_else(not_form)?;
        // Recover the generator's height interval from its actual endpoints. The
        // recogniser supplies the swap and inside-out conventions, but its fitted
        // height datum and span can retain amplified circle-centre rounding.
        let axial_anchors = match (self, surface.kind()) {
            (Self::Cylinder { .. } | Self::Cone { .. }, SurfaceKind::Cone(c)) => {
                let d = form.natural_uv_domain();
                let end = if c.rev.swap_uv {
                    Vec2::new(d.u_max(), d.v_min())
                } else {
                    Vec2::new(d.u_min(), d.v_max())
                };
                let anchor = |uv| -> Result<(Vec3, f64)> {
                    Ok((
                        form.evaluate_point(uv)?,
                        surface.convert_uv_from_nurbs_to_step(k, uv)?.y,
                    ))
                };
                Some([anchor(d.min())?, anchor(end)?])
            }
            _ => None,
        };
        // the recognised surface gives its conventions only (its frame turn, sweep, generator
        // direction and swap), as in `derive`: the kind and the numbers are this support's. The
        // numbers recognition recomputed are not compared with the support's, because it
        // recomputes them from the control points and so reproduces a support's own numbers
        // only to rounding (#2554), and far from the world origin it reads a cylinder as a cone
        // of an angle of rounding -- `lies_on` above and `evaluates_as` below are what prove
        // the two agree. A cylinder or cone takes a recognised cone's, a sphere a sphere's (a
        // torus returned above).
        // this support's own numbers on the recognised parametrisation, which the generator
        // and the sweep converter carry, so that `of` reads the support back unchanged
        match (surface.kind_mut(), *self) {
            (SurfaceKind::Cone(c), Self::Cylinder { placement, radius }) => {
                on_axis_from(&mut c.rev, placement, axial_anchors.ok_or_else(not_form)?)?;
                c.base_radius = radius;
                c.semi_angle_deg = 0.0;
                c.cylinder = true;
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
                on_axis_from(&mut c.rev, placement, axial_anchors.ok_or_else(not_form)?)?;
                c.base_radius = base_radius;
                c.semi_angle_deg = semi_angle_deg;
                c.rev.base = form.copy();
            }
            (SurfaceKind::Sphere(sp), Self::Sphere { placement, radius }) => {
                sp.base.position = placement;
                sp.radius = radius;
                sp.base.base = form.copy();
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

/// Carry the recognised generator's conventions onto the support's own axis. Its
/// endpoint heights are projected from the form, avoiding a fitted circle centre's
/// axial rounding in either the height datum or span. The original converted heights
/// determine their order for inside-out surfaces. The generator line is reparameterised
/// with the same affine map, keeping its NURBS points and matching the surface interval.
/// A placement off the axis or turned about it is still refused by `evaluates_as`.
fn on_axis_from(
    rev: &mut crate::surface::surf_of_revolution::SurfOfRevolution,
    placement: Axis2Placement,
    anchors: [(Vec3, f64); 2],
) -> Result<()> {
    use crate::curve::curve::CurveKind;
    use crate::geom::extent1d::Extent1d;
    let [(p0, h0), (p1, h1)] = anchors;
    let project = |p: Vec3| (p - placement.origin()).dot(placement.z_axis());
    let (lo, hi) = if h0 < h1 {
        (project(p0), project(p1))
    } else {
        (project(p1), project(p0))
    };
    let d = rev.anal_uv_domain;
    let old_length = d.v_max() - d.v_min();
    let new_length = hi - lo;
    if old_length <= 0.0 || new_length <= 0.0 {
        return Err(not_form());
    }
    let scale = new_length / old_length;
    if !scale.is_finite() || scale <= 0.0 {
        return Err(not_form());
    }
    let offset = lo - scale * d.v_min();
    if !offset.is_finite() {
        return Err(not_form());
    }
    rev.anal_uv_domain = Extent2d::from_bounds(d.u_min(), lo, d.u_max(), hi);
    rev.position = placement;
    match rev
        .gen_curve
        .as_mut()
        .map(crate::curve::curve::Curve::kind_mut)
    {
        Some(CurveKind::Line(line)) => {
            let line_scale = line.line_scale() / scale;
            if !line_scale.is_finite() || line_scale == 0.0 {
                return Err(not_form());
            }
            line.set_line_scale(line_scale);
            line.shift_step_parameter(offset);
            line.set_analytic_domain(Extent1d::from_min_max(lo, hi));
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
        Err(
            e @ (SolidError::Unported { .. }
            | SolidError::Cancelled { .. }
            | SolidError::OutOfFuel { .. }),
        ) => Err(e),
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

/// [`evaluates_as`]'s ratio of distance to bound between the analytic surface's STEP point
/// at `step_uv` and the point `f` a caller says the form `form` has there (infinite for a
/// distance that is not a number): at most 1 where the two agree within the rounding
/// [`evaluates_as`] derives at that STEP parameter. `None` for a surface the bound does not
/// cover. A certificate of one parameter pair, for checking an evaluation or a conversion
/// a caller made (`f` the form's point at the NURB parameter the conversion pairs with
/// `step_uv`).
///
/// # Errors
///
/// Those of the STEP evaluation and of the form's net.
pub fn evaluation_ratio_of(
    k: &Kernel,
    surface: &Surface,
    form: &BSplineSurface,
    step_uv: Vec2,
    f: Vec3,
) -> Result<Option<f64>> {
    let (net, p, q) = euclidean_net(form)?;
    let size = net.iter().flatten().fold(0.0_f64, |a, x| a.max(x.length()));
    let Some(r) = Rounding::of(surface, form.is_rational(), p, q, size, &net)? else {
        return Ok(None);
    };
    let s = surface.evaluate_step_point(k, step_uv)?;
    let ratio = (s - f).length() / r.at(step_uv, s, f);
    Ok(Some(if ratio.is_nan() { f64::INFINITY } else { ratio }))
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
            let c = cps.get(i * nv + j).ok_or_else(|| {
                SolidError::surface(
                    OP_DERIVE,
                    SurfaceFailure::Defect {
                        why: "the net has no control point (i, j) with i < nu and j < nv, though `control_points` returns the (n + 1)(m + 1) points of the net's rows, each of which holds m + 1 points by `srf_basic::Surface`'s invariant",
                    },
                )
            })?;
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

/// The circle of a rational quadratic arc from its control triangle `p0`, `p1`, `p2` (its
/// ends and its tangents' intersection): its centre and radius together from the endpoint
/// tangent and the chord, its plane's normal, and the triangle's doubled area, which says how
/// well the triangle fixes that plane (The NURBS Book, section 7.3). `None` for a degenerate
/// triangle.
fn control_circle(p0: Vec3, p1: Vec3, p2: Vec3) -> Option<(Vec3, f64, Vec3, f64)> {
    let leg = p1 - p0;
    let chord = p2 - p0;
    let tangent = leg.unitize().ok()?;
    let across = chord.unitize().ok()?;
    let normal = leg.cross(chord).unitize().ok()?;
    let radius = leg.length() * tangent.dot(across) / tangent.cross(across).length();
    let centre = p0 + normal.cross(tangent) * radius;
    (radius.is_finite() && radius > 0.0).then_some((
        centre,
        radius,
        normal,
        leg.cross(chord).length(),
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
    /// Cylinders, cones and tori take their numbers from the form's control circles;
    /// recognition supplies their conventions. A rational NURBS represents these surfaces
    /// exactly. [`AnalyticSupport::build`] checks the form against the derived surface.
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
        let recognised = recognise(k, kind, form)?
            .as_ref()
            .and_then(Self::of)
            .ok_or_else(not_kind)?;
        match (kind, recognised) {
            // a cylinder's or cone's numbers read off the form's control structure, on the
            // recogniser's conventions; a cylinder the recogniser reads as a cone of an angle
            // of rounding (1e-10 degrees far from the origin) is the cylinder its rows are
            (SupportKind::Cylinder, Self::Cylinder { .. } | Self::Cone { .. }) => {
                Self::revolved_from_rows(recognised, true, &net, form).ok_or_else(not_kind)
            }
            (SupportKind::Cone, Self::Cone { .. }) => {
                Self::revolved_from_rows(recognised, false, &net, form).ok_or_else(not_kind)
            }
            (SupportKind::Torus, Self::Torus { placement, .. }) => {
                // Use a Bezier patch: after knot insertion the first three controls of
                // the whole net need not be an arc's endpoints and tangent intersection.
                let patches = decompose_to_bez(form.gw_nurb().ok_or_else(not_kind)?)?;
                let patch = patches
                    .first()
                    .and_then(|row| row.first())
                    .ok_or_else(not_kind)?;
                let net: Vec<Vec<Vec3>> = patch
                    .rows()
                    .map(|row| {
                        row.iter()
                            .map(|c| {
                                let w = if c.w == NOW { 1.0 } else { c.w };
                                Vec3::new(c.x, c.y, c.z) * (1.0 / w)
                            })
                            .collect()
                    })
                    .collect();
                Self::torus_from_net(placement, &net).ok_or_else(not_kind)
            }
            (kind, other) if other.kind() == kind => Ok(other),
            _ => Err(not_kind()),
        }
    }

    /// A torus's circles from its quadratic boundary control triangles, rather than a fit
    /// through evaluated points. The sweep circle fixes the axis; the meridian circle
    /// fixes the tube centre and radius together (The NURBS Book, sections 7.3 and 7.5).
    fn torus_from_net(placement: Axis2Placement, net: &[Vec<Vec3>]) -> Option<Self> {
        let at = |i: usize, j: usize| net.get(i).and_then(|r| r.get(j)).copied();
        let circle = control_circle;
        let boundary = |along_u: bool, fixed: usize| {
            if along_u {
                circle(at(0, fixed)?, at(1, fixed)?, at(2, fixed)?)
            } else {
                circle(at(fixed, 0)?, at(fixed, 1)?, at(fixed, 2)?)
            }
        };
        // Either boundary sweep can collapse on the axis. Use the boundary with
        // the larger control triangle to recover each direction's plane robustly.
        let strongest = |along_u| {
            [0, 2]
                .into_iter()
                .filter_map(|fixed| boundary(along_u, fixed))
                .max_by(|a, b| a.3.total_cmp(&b.3))
        };
        let a = strongest(true)?;
        let b = strongest(false)?;
        let sweep_along_u = a.2.dot(placement.z_axis()).abs() >= b.2.dot(placement.z_axis()).abs();
        let sweep = if sweep_along_u { a } else { b };
        // Keep the meridian at the start of the sweep: its centre defines the
        // support's radial direction, including when its first point is on-axis.
        let tube = boundary(!sweep_along_u, 0)?;
        let mut axis = sweep.2;
        if axis.dot(placement.z_axis()) < 0.0 {
            axis = -axis;
        }
        let origin = sweep.0 + axis * (tube.0 - sweep.0).dot(axis);
        let radial = tube.0 - origin;
        let major_radius = radial.length();
        let x = radial.unitize().ok()?;
        Some(Self::Torus {
            placement: Axis2Placement::from_stored(origin, x, axis.cross(x)),
            major_radius,
            minor_radius: tube.1,
        })
    }

    /// A cylinder's or cone's numbers read straight off its NURBS form's control structure
    /// (#15010): the form's circles are its rows across the generator, each a rational arc
    /// whose chord and middle weight recover its centre and radius together (The NURBS Book,
    /// §7.3 and §7.5: the weight is the cosine of the half sweep). A cylinder
    /// keeps that pair from the first row; averaging independently recovered radii breaks
    /// their correlated rounding on shallow arcs. The axis is a cylinder's columns' direction
    /// (its generators) through the first row's centre, or a cone's circular-row normal. The
    /// origin is the point of the axis nearest the recogniser's, the x axis the recogniser's
    /// made square to it, and the cone's angle the radii's change along it. No fitting and
    /// no long chains: the numbers are the stored control points' to their rounding. The
    /// recognisers' chained computations tilt the axis by about 1e-13 (an ulp of the origin's
    /// size), which a form
    /// far from the world origin turns into a parametrisation `build` refuses. The
    /// recogniser's frame turn, sweep, generator direction and swap are kept: they are
    /// conventions, not numbers. A row collapsed to one point (a cone's apex) is the axis's
    /// point of radius 0. `None` where the form gives no such rows (fewer than two circles,
    /// or a degenerate tangent and chord).
    fn revolved_from_rows(
        recognised: Self,
        cylinder: bool,
        net: &[Vec<Vec3>],
        form: &BSplineSurface,
    ) -> Option<Self> {
        let (placement_r, semi_r) = match recognised {
            Self::Cylinder { placement, .. } => (placement, 0.0),
            Self::Cone {
                placement,
                semi_angle_deg,
                ..
            } => (placement, semi_angle_deg),
            _ => return None,
        };
        let axis_r = placement_r.z_axis();
        let (nu, nv) = (net.len(), net.first().map_or(0, Vec::len));
        let at = |i: usize, j: usize| net.get(i).and_then(|r| r.get(j)).copied();
        // the circles run along the direction whose rows stay square to the axis
        let spread = |along_v: bool| {
            let (rows, len) = if along_v { (nu, nv) } else { (nv, nu) };
            let mut worst = 0.0_f64;
            for r in 0..rows {
                let first = if along_v { at(r, 0) } else { at(0, r) };
                for c in 0..len {
                    let p = if along_v { at(r, c) } else { at(c, r) };
                    if let (Some(p), Some(f)) = (p, first) {
                        worst = worst.max((p - f).dot(axis_r).abs());
                    }
                }
            }
            worst
        };
        let along_v = spread(true) <= spread(false);
        let (rows, len) = if along_v { (nu, nv) } else { (nv, nu) };
        // Each row's circle from its first rational quadratic arc, control points p0, p1,
        // p2 (The NURBS Book, sections 7.3 and 7.5). The angle α between the tangent leg
        // and the chord is the half sweep. A wide span recovers the centre from that leg,
        // which is exact for a circle of an exactly representable radius (a line at radius
        // 5 revolved about the axis). A short span's leg is nearly parallel to the chord,
        // and the centre built from the leg sits off the axis: 6.8×10⁻¹³ mm on the 12° rib,
        // 1.06 times the rounding `build` allows. There the middle weight w1/√(w0 w2) is
        // cos α, the radius is the chord over 2 sin α, and the centre lies r |cos α| from
        // the chord's midpoint. That puts the 12° rib 2.7×10⁻¹⁴ mm off.
        let degree = form.degree(if along_v {
            crate::nurbs::b_spline_surface::SurfParamType::V
        } else {
            crate::nurbs::b_spline_surface::SurfParamType::U
        });
        if degree != 2 || len < 3 {
            return None;
        }
        let (_, nv_cp, cps) = form.control_points().ok()?;
        let circle = |r: usize| -> Option<(Vec3, f64, Vec3)> {
            let point = |c: usize| if along_v { at(r, c) } else { at(c, r) };
            let (p0, p1, p2) = (point(0)?, point(1)?, point(2)?);
            // a row collapsed to one point is a cone's apex: a point of the axis, of radius
            // 0. Its control points are one stored point, apart only by the rounding of
            // dividing each homogeneous point by its weight (`euclidean_net`: within
            // `gamma_4` of its size)
            let size = (0..len)
                .filter_map(point)
                .fold(0.0_f64, |a, q| a.max(q.length()));
            let collapsed = (0..len)
                .filter_map(point)
                .all(|q| (q - p0).length() <= gamma(4) * size);
            if collapsed {
                return Some((p0, 0.0, Vec3::ZERO));
            }
            let leg = p1 - p0;
            let chord = p2 - p0;
            let normal = leg.cross(chord);
            let unit = |v: Vec3| {
                let length = v.length();
                (length > 0.0 && length.is_finite()).then(|| v * (1.0 / length))
            };
            let tangent = unit(leg)?;
            let across = unit(chord)?;
            // sin α. At about 15° and above the leg is a stable reading of the circle.
            // Below that the chord and the weight are.
            let sine = tangent.cross(across).length();
            let (centre, radius) = if sine >= 0.25 {
                let inward = unit(normal)?.cross(tangent);
                let radius = leg.length() * tangent.dot(across) / sine;
                (p0 + inward * radius, radius)
            } else {
                let plane = unit(normal)?;
                let at_cp = |c: usize| {
                    let (i, j) = if along_v { (r, c) } else { (c, r) };
                    let index = i.checked_mul(nv_cp)?.checked_add(j)?;
                    cps.get(index)
                };
                let weight_of = |c: &crate::nurbs::math::CPoint| if c.w == NOW { 1.0 } else { c.w };
                let (w0, w1, w2) = (
                    weight_of(at_cp(0)?),
                    weight_of(at_cp(1)?),
                    weight_of(at_cp(2)?),
                );
                // The end weights share a sign and the middle one is not zero. cos α is
                // w1/√(w0 w2), negative for a span past 180° and for a row whose weights
                // are all negated. Both are the same circle. The control triangle's normal
                // turns over with that row, and the centre offset uses |cos α|.
                if !(w0 * w2 > 0.0 && w1.is_finite() && w1 != 0.0) {
                    return None;
                }
                let cos_half = w1 / (w0 * w2).sqrt();
                let sin_half = ((1.0 - cos_half) * (1.0 + cos_half)).sqrt();
                if !(sin_half.is_finite() && sin_half > 0.0) {
                    return None;
                }
                let radius = chord.length() / (2.0 * sin_half);
                let centre = p0 + chord * 0.5 + plane.cross(across) * (radius * cos_half.abs());
                (centre, radius)
            };
            (centre.x.is_finite() && centre.y.is_finite() && centre.z.is_finite() && radius > 0.0)
                .then_some((centre, radius, normal))
        };
        let circles: Vec<(Vec3, f64, Vec3)> = (0..rows).filter_map(circle).collect();
        if circles.len() < 2 {
            return None;
        }
        let (&(c0, r0, _), &(c1, r1, _)) = (circles.first()?, circles.last()?);
        let along = c1 - c0;
        // a cylinder's axis runs along its generators, which are the form's columns: each
        // column's first and last control points differ by the stored extrusion, exactly for
        // an extruded circle. Deriving an axis from the circle centres can amplify their
        // rounding: on a 20 mm arc of radius 77.6 mm the old centres' line tilted by
        // 1.3e-14, which `build` refused. A cone's
        // axis is normal to its circular rows. Use the largest control-polygon normal
        // to avoid amplifying the centres' rounding by the rows' short separation.
        let axis_vector = if cylinder {
            let point = |r: usize, c: usize| if along_v { at(r, c) } else { at(c, r) };
            let last = rows.checked_sub(1)?;
            (0..len).try_fold(Vec3::new(0.0, 0.0, 0.0), |g, c| {
                Some(g + (point(last, c)? - point(0, c)?))
            })?
        } else {
            circles
                .iter()
                .map(|&(_, _, normal)| normal)
                .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))?
        };
        let length = axis_vector.length();
        if length.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
            return None;
        }
        let mut axis = axis_vector * (1.0 / length);
        if axis.dot(axis_r) < 0.0 {
            axis *= -1.0;
        }
        let origin = c0 + axis * (placement_r.origin() - c0).dot(axis);
        let xr = placement_r.x_axis();
        let x = xr - axis * xr.dot(axis);
        let x = x * (1.0 / x.length());
        let y = axis.cross(x);
        let placement = Axis2Placement::from_stored(origin, x, y);
        let finite = [origin, x, y]
            .iter()
            .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite());
        if !finite {
            return None;
        }
        if cylinder {
            // Keep the first row's centre and radius together. On a shallow arc their
            // rounding is correlated: averaging radii from other rows while retaining c0
            // moves the cylinder off its own endpoints, even with joint circle recovery.
            return Some(Self::Cylinder {
                placement,
                radius: r0,
            });
        }
        // the radius changes by tan(angle) per unit along the axis
        let tan = (r1 - r0) / along.dot(axis);
        let semi_angle_deg = tan.atan().to_degrees();
        if semi_r != 0.0 && (semi_angle_deg > 0.0) != (semi_r > 0.0) {
            return None;
        }
        let base_radius = r0 + tan * (origin - c0).dot(axis);
        Some(Self::Cone {
            placement,
            base_radius,
            semi_angle_deg,
        })
    }

    /// Not a support's kind (#2485): the surface of revolution a NURBS form is, its numbers
    /// read off the form's control structure ([`Self::revolution_from_rows`]) on the
    /// conventions the kernel recognises in the form (`IsNurbSurfaceSurfOfRevolution`, which
    /// `CopyAndAddAnalytics` runs: its generator, generator range, swap and axis sense); it
    /// takes `form` as its NURB and must evaluate as the form at the form's own parameters,
    /// as [`AnalyticSupport::build`] requires of a support.
    ///
    /// # Errors
    ///
    /// `InvalidInput` where the form is no surface of revolution, has no circular control
    /// row, or is not the derived one's parametrisation; those of the recogniser, the
    /// derivation and the evaluations.
    pub fn derive_revolution(k: &Kernel, form: &BSplineSurface) -> Result<Surface> {
        use crate::surface::surf_of_revolution::SurfOfRevolution;
        let not_kind = || {
            SolidError::invalid_input(
                OP_DERIVE,
                "an exact net's NURBS form is not a surface of revolution's parametrisation",
            )
        };
        let test = Surface::from(form.copy());
        let recognised = match SurfOfRevolution::is_nurb_surface_surf_of_revolution(k, &test, 1.0) {
            Ok(Some(rev)) => rev,
            Err(
                e @ (SolidError::Unported { .. }
                | SolidError::Cancelled { .. }
                | SolidError::OutOfFuel { .. }),
            ) => return Err(e),
            Ok(None) | Err(_) => return Err(not_kind()),
        };
        let surface = Self::revolution_from_rows(k, &recognised, form)?.ok_or_else(not_kind)?;
        if !evaluates_as(k, &surface, form)? {
            return Err(not_kind());
        }
        Ok(surface)
    }

    /// The surface of revolution `recognised` in `form`, its numbers read off the form's
    /// control structure rather than the recogniser's fit, as a cylinder's, cone's and torus's
    /// are ([`Self::revolved_from_rows`], [`Self::torus_from_net`]). The recogniser's axis is
    /// the circle's through three evaluated points of the first sweep (`is_arc`); on a sweep of
    /// a fraction of a degree, 0.45 degrees at a radius of 15.6 mm in
    /// tangency-labs/solidcraft#17004, that centre lies 4e-11 off sideways and turns the swept
    /// points past the rounding [`evaluates_as`] forgives.
    ///
    /// Every control row across the sweep is a circle, a generator control point turned about
    /// the axis. Of the rows of the form's Bezier patches, the one whose first sweep span has
    /// the largest control triangle gives the numbers ([`control_circle`]; a row on the axis has
    /// none): the axis is the triangle's normal, in the recogniser's sense; the centre is the
    /// triangle's, moved onto the chord's perpendicular bisector. A short arc's control points
    /// fix the centre's distance along the bisector only to their rounding over the arc's angle
    /// squared, but the bisector itself to that over the angle; a centre off along the bisector
    /// moves the swept points by the square of the sweep, one off across it turns them by the
    /// sweep. The x axis points to the start of the sweep circle on which the surface's polar
    /// converter measures the NURB's angles (`RebuildPolarConverter`: the sweep iso at the
    /// generator's start, or at 0.45 of its range where that one lies on the axis), and the
    /// sweep ends at the angle of that circle's end. Another row's start is turned from it by
    /// the net's rounding, and every converted angle with it: 5e-14 radians between the
    /// generator's ends on a 286 degree sweep in #17004's body. The recogniser's generator,
    /// generator range and swap are kept, and its origin's height on the axis: they are
    /// conventions, not numbers. `None` where no row has a triangle or both sweep isos lie on
    /// the axis.
    ///
    /// # Errors
    ///
    /// Those of the form's control net and Bezier decomposition, its sweep iso and that one's
    /// evaluation, the generator's copy,
    /// [`SurfOfRevolution::new`](crate::surface::surf_of_revolution::SurfOfRevolution::new)
    /// and its reparameterisation.
    fn revolution_from_rows(
        k: &Kernel,
        recognised: &crate::surface::surf_of_revolution::SurfOfRevolution,
        form: &BSplineSurface,
    ) -> Result<Option<Surface>> {
        use crate::math::EFF_ZERO;
        use crate::nurbs::b_spline_surface::{IsoCurveOptions, SurfParamType};
        use crate::surface::surf_of_revolution::SurfOfRevolution;
        let Some(nurb) = form.gw_nurb() else {
            return Ok(None);
        };
        let euclid = |c: &crate::nurbs::math::CPoint| {
            let w = if c.w == NOW { 1.0 } else { c.w };
            Vec3::new(c.x, c.y, c.z) * (1.0 / w)
        };
        let patches: Vec<Vec<Vec<Vec<Vec3>>>> = decompose_to_bez(nurb)?
            .iter()
            .map(|row| {
                row.iter()
                    .map(|patch| {
                        patch
                            .rows()
                            .map(|r| r.iter().map(euclid).collect())
                            .collect()
                    })
                    .collect()
            })
            .collect();
        // the sweep runs along u unless the recogniser swapped it
        let sweep_along_u = !recognised.swap_uv;
        // the patch of the first span along the sweep and span `g` along the generator
        let patch = |g: usize| -> Option<&Vec<Vec<Vec3>>> {
            if sweep_along_u {
                patches.first()?.get(g)
            } else {
                patches.get(g)?.first()
            }
        };
        // a patch's control point `j` along the sweep of its generator row `r`
        let at = |net: &Vec<Vec<Vec3>>, r: usize, j: usize| -> Option<Vec3> {
            if sweep_along_u {
                net.get(j)?.get(r).copied()
            } else {
                net.get(r)?.get(j).copied()
            }
        };
        let generator_spans = if sweep_along_u {
            patches.first().map_or(0, Vec::len)
        } else {
            patches.len()
        };
        let mut best: Option<(f64, Vec3, Vec3, Vec3, Vec3)> = None;
        for g in 0..generator_spans {
            let Some(first) = patch(g) else {
                return Ok(None);
            };
            let rows = if sweep_along_u {
                first.first().map_or(0, Vec::len)
            } else {
                first.len()
            };
            for r in 0..rows {
                let (Some(p0), Some(p1), Some(p2)) =
                    (at(first, r, 0), at(first, r, 1), at(first, r, 2))
                else {
                    return Ok(None);
                };
                let Some((centre, _, normal, area)) = control_circle(p0, p1, p2) else {
                    continue;
                };
                if best.as_ref().is_none_or(|b| area > b.0) {
                    best = Some((area, centre, normal, p0, p2));
                }
            }
        }
        let Some((_, centre, normal, p0, p2)) = best else {
            return Ok(None);
        };
        // the sweep circle the polar converter measures the NURB's angles on
        // (`RebuildPolarConverter`): the sweep iso at the generator's start, or at 0.45 of
        // its range where that one lies on the axis
        let uv = form.natural_uv_domain();
        let sweep_iso = |fraction: f64| -> Result<BSplineCurve> {
            let at = uv.evaluate(fraction, fraction);
            let (param, value) = if sweep_along_u {
                (SurfParamType::V, at.y)
            } else {
                (SurfParamType::U, at.x)
            };
            Ok(form
                .create_iso_parametric_curve(k, param, value, IsoCurveOptions::default())?
                .curve)
        };
        let mut circle = sweep_iso(0.0)?;
        if circle.is_degenerate(k, EFF_ZERO, None)? {
            circle = sweep_iso(0.45)?;
            if circle.is_degenerate(k, EFF_ZERO, None)? {
                return Ok(None);
            }
        }
        let ends = circle.natural_interval();
        let (start, stop) = (
            circle.evaluate_point(ends.min())?,
            circle.evaluate_point(ends.max())?,
        );
        let position = recognised.position;
        let axis = if normal.dot(position.z_axis()) < 0.0 {
            -normal
        } else {
            normal
        };
        let mid = (p0 + p2) * 0.5;
        let Ok(bisector) = axis.cross(p2 - p0).unitize() else {
            return Ok(None);
        };
        let centre = mid + bisector * (centre - mid).dot(bisector);
        let radial = |p: Vec3| {
            let d = p - centre;
            d - axis * d.dot(axis)
        };
        let Ok(x) = radial(start).unitize() else {
            return Ok(None);
        };
        let y = axis.cross(x);
        let end = radial(stop);
        let domain = recognised.anal_uv_domain;
        // the turn nearest the recognised sweep's: a sweep past 180 degrees
        let mut sweep = end.dot(y).atan2(end.dot(x)).to_degrees();
        sweep += 360.0 * ((domain.max().x - domain.min().x - sweep) / 360.0).round();
        let origin = centre + axis * (position.origin() - centre).dot(axis);
        let finite = [origin, x, y]
            .iter()
            .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite());
        if !finite || !sweep.is_finite() || sweep <= 0.0 {
            return Ok(None);
        }
        let Some(generator) = recognised.gen_curve.as_ref() else {
            return Ok(None);
        };
        let mut rev = SurfOfRevolution::new(
            k,
            generator.copy(k)?,
            origin,
            x,
            y,
            Extent2d::from_bounds(0.0, domain.min().y, sweep, domain.max().y),
            recognised.swap_uv,
        )?;
        // (as the recogniser: the NURB's domain the form's, status ignored)
        crate::error::ignore_status(rev.reparameterize(k, &form.natural_uv_domain()))?;
        rev.base = form.copy();
        Ok(Some(Surface::from(rev)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Point3;
    use crate::nurbs::b_spline_curve::KnotType;
    use crate::nurbs::b_spline_surface::{BSplineSurfaceForm, CanonicalSurfaceData};
    use crate::primitive::api_primitives as ap;

    /// The NURBS form of an analytic surface, and its implicit equation.
    fn form_and_equation(k: &Kernel, s: &Surface) -> (BSplineSurface, Implicit) {
        let form = crate::blend::fillet_section::station::as_b_spline(k, s)
            .unwrap()
            .unwrap();
        (
            form.into_owned(),
            AnalyticSupport::of(s).unwrap().implicit(),
        )
    }

    /// The cone about Z through the origin, radius `radius` there.
    fn cone(radius: f64, tan_angle: f64) -> Implicit {
        Implicit::Cone {
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            radius,
            tan_angle,
        }
    }

    /// [`off_surface_bound`] of a plane patch a known distance off a plane: the bilinear
    /// patch at x = 2 (y, z from 0 to 1) lies 0.5 from the plane x = 1.5, and its equation
    /// is linear in the homogeneous coordinates (weights 1), so the bound is 0.5 within a
    /// few roundings (PROVEN: never below the true distance), and on the plane x = 2 only
    /// the counted rounding (the slack `2 γ₄₈ · 4 · 2 ≈ 8.5e-14`, `4 = |H|∞ + |O|∞ W`, plus
    /// the coefficients' `τ = γ₃₂ · 4 ≈ 1.4e-14`, under 2e-13). Not a number in a control
    /// point or in the equation has no bound. Cut from a net a million times larger, the
    /// same patch carries that net's rounding: at least `2 γ₄₈ · 2e6 · 2 ≈ 4.3e-8`.
    #[test]
    fn a_plane_patch_off_a_plane_is_bounded_by_its_distance() {
        let patch = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &[
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(2.0, 1.0, 0.0),
                Point3::new(2.0, 0.0, 1.0),
                Point3::new(2.0, 1.0, 1.0),
            ],
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 4]),
            uv_domain: None,
        })
        .unwrap();
        let plane = |x: f64| Implicit::Plane {
            origin: Vec3::new(x, 0.0, 0.0),
            normal: Vec3::X,
        };
        let off = off_surface_bound(&patch, &plane(1.5), &patch)
            .unwrap()
            .unwrap();
        assert!((0.5..=0.5 * (1.0 + 1e-12)).contains(&off), "{off:e}");
        let on = off_surface_bound(&patch, &plane(2.0), &patch)
            .unwrap()
            .unwrap();
        assert!((0.0..=2e-13).contains(&on), "{on:e}");
        let large = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &[
                Point3::new(2e6, 0.0, 0.0),
                Point3::new(2e6, 1.0, 0.0),
                Point3::new(2e6, 0.0, 1.0),
                Point3::new(2e6, 1.0, 1.0),
            ],
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 4]),
            uv_domain: None,
        })
        .unwrap();
        let from_large = off_surface_bound(&patch, &plane(2.0), &large)
            .unwrap()
            .unwrap();
        assert!(from_large >= 2.0 * gamma(48) * 2e6 * 2.0, "{from_large:e}");
        // (not a number in the equation, or in a control point)
        let nan_plane = Implicit::Plane {
            origin: Vec3::new(f64::NAN, 0.0, 0.0),
            normal: Vec3::X,
        };
        assert_eq!(off_surface_bound(&patch, &nan_plane, &patch).unwrap(), None);
        assert_eq!(
            off_surface_bound(&patch, &cone(1.0, f64::NAN), &patch).unwrap(),
            None
        );
        let nan_patch = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &[
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(f64::NAN, 1.0, 0.0),
                Point3::new(2.0, 0.0, 1.0),
                Point3::new(2.0, 1.0, 1.0),
            ],
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 4]),
            uv_domain: None,
        });
        // (a patch so small that its coefficients' rounding `τ` is below
        // `MIN_POSITIVE / ε`, where underflow is outside the `γ` model: no bound)
        let tiny = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &[
                Point3::new(2e-300, 0.0, 0.0),
                Point3::new(2e-300, 1e-300, 0.0),
                Point3::new(2e-300, 0.0, 1e-300),
                Point3::new(2e-300, 1e-300, 1e-300),
            ],
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 4]),
            uv_domain: None,
        })
        .unwrap();
        let tiny_plane = Implicit::Plane {
            origin: Vec3::new(1.5e-300, 0.0, 0.0),
            normal: Vec3::X,
        };
        assert_eq!(off_surface_bound(&tiny, &tiny_plane, &tiny).unwrap(), None);
        // (not a number in one coordinate of a source point outside the patch, the others
        // finite: `max` would hide it, and the source's size would not be a number)
        let mut source = patch.gw_nurb().unwrap().clone();
        source.cpt_mut(0, 1).unwrap().x = f64::NAN;
        let source = BSplineSurface::from_nurb(source, true);
        assert_eq!(
            off_surface_bound(&patch, &plane(1.5), &source).unwrap(),
            None
        );
        // (a surface that cannot be made from it is refused earlier, as well)
        if let Ok(nan_patch) = nan_patch {
            assert_eq!(
                off_surface_bound(&nan_patch, &plane(2.0), &nan_patch).unwrap_or(None),
                None
            );
        }
    }

    /// [`off_surface_bound`] of a cylinder r 50, z 0 to 100, as the kernel stores it (a
    /// rational form, exact in exact arithmetic): within roundings of 0 against its own
    /// equation, and against the cylinders r 50 ∓ 1e-3 about the same axis, which every
    /// point of it lies exactly 1e-3 from, at least 1e-3 (PROVEN: never below the true
    /// distance) and at most 5e-3: `|ρ² − ρ_P²| / ρ_P` is `(ρ + ρ_P) / ρ_P ≈ 2` times the
    /// distance, and the rational form's weights 1 and √2/2 give coefficients up to `W₀⁻² =
    /// 2` times the values (ARGUED, so about 4e-3).
    #[test]
    fn a_cylinder_is_bounded_by_its_distance_from_a_coaxial_cylinder() {
        let k = Kernel::default();
        let s = Surface::from(ap::create_cylinder_surface(&k, Point3::ZERO, 50.0, 100.0).unwrap());
        let (form, own) = form_and_equation(&k, &s);
        let on = off_surface_bound(&form, &own, &form).unwrap().unwrap();
        assert!((0.0..=1e-9).contains(&on), "{on:e}");
        for r in [50.0 - 1e-3, 50.0 + 1e-3] {
            let off = off_surface_bound(&form, &cone(r, 0.0), &form)
                .unwrap()
                .unwrap();
            assert!((1e-3..=5e-3).contains(&off), "{r} {off:e}");
        }
    }

    /// [`off_surface_bound`] of a frustum r 50 at z 0 to r 30 at z 100 as the kernel
    /// stores it: within roundings of 0 against its own equation; against the cone of the
    /// same axis and angle shifted out by 1e-3 in radius (every point of the frustum is
    /// 1e-3 · cos(semi-angle) from it), at least that distance (PROVEN) and at most eight
    /// times it: `|ρ² − ρ_P²|` is up to `(50 + 50)` times the radial offset and is divided
    /// by the least radius 30, the weights give up to 2 more, and the radial offset is
    /// `√(1 + t²)` times the distance (ARGUED, so about 6.8; MEASURED 6.80). A sphere's
    /// equation has no bound (`None`).
    #[test]
    fn a_cone_is_bounded_by_its_distance_from_a_shifted_cone() {
        let k = Kernel::default();
        let s =
            Surface::from(ap::create_cone_surface(&k, Point3::ZERO, 50.0, 30.0, 100.0).unwrap());
        let (form, own) = form_and_equation(&k, &s);
        let on = off_surface_bound(&form, &own, &form).unwrap().unwrap();
        assert!((0.0..=1e-9).contains(&on), "{on:e}");
        let Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        } = own
        else {
            panic!("{own:?}");
        };
        let shifted = Implicit::Cone {
            origin,
            axis,
            radius: radius + 1e-3,
            tan_angle,
        };
        let d = 1e-3 / (1.0 + tan_angle * tan_angle).sqrt();
        let off = off_surface_bound(&form, &shifted, &form).unwrap().unwrap();
        assert!(d <= off && off <= 8.0 * d, "{d:e} {off:e}");
        let sphere = Implicit::Sphere {
            centre: Vec3::ZERO,
            radius: 50.0,
        };
        assert_eq!(off_surface_bound(&form, &sphere, &form).unwrap(), None);
    }

    /// A homogeneous point and the length of the longest chain of
    /// [`CPoint::combine2`](crate::nurbs::math::CPoint::combine2) levels behind it.
    type Tracked = (crate::nurbs::math::CPoint, usize);

    fn mix(a: f64, x: Tracked, b: f64, y: Tracked) -> Tracked {
        (
            crate::nurbs::math::CPoint::combine2(a, x.0, b, y.0),
            x.1.max(y.1) + 1,
        )
    }

    /// One column (or row) of `extract_patch` over `[a, b]`, its loops as written there
    /// (`a`, `b` inside the domain and off the knots, so nothing snaps).
    fn extract_line(pts: &[Tracked], p: usize, u: &[f64], a: f64, b: f64) -> Vec<Tracked> {
        use crate::nurbs::knots::{KnotVector, Side};
        let kv = KnotVector::from_slice(u).unwrap();
        let (span_l, mul) = kv.find_span_and_mult(p, a, Side::Left).unwrap();
        let (span_r, mur) = kv.find_span_and_mult(p, b, Side::Left).unwrap();
        let (p, span_l, mul, span_r, mur) = (
            p as isize,
            span_l as isize,
            mul as isize,
            span_r as isize,
            mur as isize,
        );
        let at = |i: isize| u[i as usize];
        let (is, ie) = (span_l - p, span_r - mur);
        let n = ie - is;
        let mut aw: Vec<Tracked> = pts[is as usize..=ie as usize].to_vec();
        let ll = span_l - p;
        for i in 1..=p - mul {
            for j in 0..=p - i - mul {
                let left = at(ll + i + j);
                let alf = (a - left) / (at(span_l + j + 1) - left);
                let (j, oma) = (j as usize, 1.0 - alf);
                aw[j] = mix(alf, aw[j + 1], oma, aw[j]);
            }
        }
        let (lr, lk) = (span_r - p, n - p + mur);
        for i in 1..=p - mur {
            let mut j = p - i - mur;
            while j >= 0 {
                let k = (lk + i + j) as usize;
                let left = at(lr + i + j).max(a);
                let alf = (b - left) / (at(span_r + j + 1) - left);
                aw[k] = mix(alf, aw[k], 1.0 - alf, aw[k - 1]);
                j -= 1;
            }
        }
        aw
    }

    /// One column (or row) of [`decompose_to_bez`], its loops as written there: the
    /// Bézier segments.
    fn decompose_line(pts: &[Tracked], p: usize, u: &[f64]) -> Vec<Vec<Tracked>> {
        let r = u.len() - 1;
        let mut bw: Vec<Tracked> = pts[..=p].to_vec();
        let mut nbw = bw.clone();
        let (mut al, mut om) = (vec![0.0; p], vec![0.0; p]);
        let (mut isu, mut ieu) = (p, p + 1);
        let mut out = Vec::new();
        while ieu < r {
            let i0 = ieu;
            while ieu < r && u[ieu] == u[ieu + 1] {
                ieu += 1;
            }
            let mlu = ieu - i0 + 1;
            // (the last strip ends at the clamped end, `mlu = p + 1`: no insertion, and
            // nothing is saved)
            let ru = p.saturating_sub(mlu);
            if mlu < p {
                let num = u[ieu] - u[isu];
                for i in (mlu + 1..=p).rev() {
                    let x = num / (u[isu + i] - u[isu]);
                    al[i - mlu - 1] = x;
                    om[i - mlu - 1] = 1.0 - x;
                }
                for i in 1..=ru {
                    let su = mlu + i;
                    for j in (su..=p).rev() {
                        bw[j] = mix(al[j - su], bw[j], om[j - su], bw[j - 1]);
                    }
                    if ieu < r {
                        nbw[ru - i] = bw[p];
                    }
                }
            }
            out.push(bw.clone());
            if ieu < r {
                for i in ru..=p {
                    nbw[i] = pts[ieu - p + i];
                }
            }
            isu = ieu;
            ieu += 1;
            core::mem::swap(&mut bw, &mut nbw);
        }
        out
    }

    /// The deepest chain over the Bézier patches of `net` (rows u, columns v), decomposed
    /// as [`decompose_to_bez`] does, each point checked bit-equal to `real`'s.
    fn deepest(
        net: &[Vec<Tracked>],
        p: usize,
        q: usize,
        u: &[f64],
        v: &[f64],
        real: &[Vec<crate::nurbs::srf_basic::Surface>],
    ) -> usize {
        let cols: Vec<Vec<Vec<Tracked>>> = (0..net[0].len())
            .map(|l| {
                let col: Vec<Tracked> = net.iter().map(|row| row[l]).collect();
                decompose_line(&col, p, u)
            })
            .collect();
        let mut depth = 0;
        for (iu, real_row) in real.iter().enumerate() {
            assert_eq!(cols[0].len(), real.len());
            for k in 0..=p {
                let row: Vec<Tracked> = cols.iter().map(|c| c[iu][k]).collect();
                let segs = decompose_line(&row, q, v);
                assert_eq!(segs.len(), real_row.len());
                for (seg, patch) in segs.iter().zip(real_row) {
                    let got: Vec<_> = patch.rows().nth(k).unwrap().to_vec();
                    let want: Vec<_> = seg.iter().map(|t| t.0).collect();
                    assert_eq!(got, want, "p {p} q {q} strip {iu} row {k}");
                    depth = seg.iter().fold(depth, |d, t| d.max(t.1));
                }
            }
        }
        depth
    }

    /// B7 of #18612's review: [`off_surface_bound`]'s `γ` covers the longest chain of
    /// knot-insertion levels its patches carry. For degrees 1 to 10 in u and v, on
    /// nets with simple interior knots (where the decomposition's chains through saved
    /// points are longest, `p (p − 1) / 2`) and with knots of mixed multiplicity, the
    /// loops of `extract_patch` and [`decompose_to_bez`] run on tracked points (bit-equal
    /// to the real ones), from the net itself (`form` as its own `source`) and from every
    /// patch cut from inside one span to inside the same or a later one. The deepest chain
    /// `L` must be within [`insertion_levels`] and `γ`'s index at least `8 L + 9` (eight
    /// roundings a level, one for the size's feedback, eight for the coefficients'
    /// scale). MEASURED: simple knots give the decomposition alone exactly
    /// `p (p − 1) / 2 + q (q − 1) / 2`, the cut patches 4 levels at `(1, 1)` (the count's
    /// 4), 41 at `(9, 1)` and 96 at `(10, 10)` (counted 130). The count before r4,
    /// `32 (p + q) + 8` from `L ≤ 4 (p + q) − 4`, fails it from `(1, 9)` and `(9, 1)`.
    #[test]
    fn the_rounding_count_covers_the_insertion_chains() {
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as Net;
        use crate::nurbs::srf_tool::p1::extract_patch;
        // clamped knots on [0, spans] with interior multiplicities `mults`
        let knots = |p: usize, mults: &[usize]| {
            let mut u = vec![0.0; p + 1];
            for (i, &m) in mults.iter().enumerate() {
                let t = (i + 1) as f64 + 0.13 * ((i * 7) % 5) as f64;
                u.extend(std::iter::repeat_n(t, m.min(p)));
            }
            let end = (mults.len() + 1) as f64 + 0.5;
            u.extend(std::iter::repeat_n(end, p + 1));
            u
        };
        let mut worst = 0;
        for p in 1..=10_usize {
            for q in 1..=10_usize {
                let simple = |d: usize| vec![1; d + 2];
                let mixed = |d: usize| (0..d + 3).map(|i| [1, 2, 1, 3][i % 4]).collect::<Vec<_>>();
                for (mu, mv) in [(simple(p), simple(q)), (mixed(p), mixed(q))] {
                    let (u, v) = (knots(p, &mu), knots(q, &mv));
                    let (rows, cols) = (u.len() - p - 1, v.len() - q - 1);
                    let net: Vec<Vec<CPoint>> = (0..rows)
                        .map(|i| {
                            (0..cols)
                                .map(|j| {
                                    let (x, y) = (i as f64, j as f64);
                                    let w = 1.0 + 0.25 * (1.3 * x + 0.7 * y).sin().abs();
                                    CPoint::new(
                                        w * (x + 0.1 * y.sin()),
                                        w * (y + 0.1 * x.cos()),
                                        w * (0.3 * x * y).sin(),
                                        w,
                                    )
                                })
                                .collect()
                        })
                        .collect();
                    let tracked: Vec<Vec<Tracked>> = net
                        .iter()
                        .map(|row| row.iter().map(|&c| (c, 0)).collect())
                        .collect();
                    let source = Net::new(net, p, q, u.clone(), v.clone()).unwrap();
                    // the net decomposed as it is
                    let own = deepest(&tracked, p, q, &u, &v, &decompose_to_bez(&source).unwrap());
                    // patches cut from it, each from inside a span to inside a later one
                    // (or the same), the same choice in u and v: the extraction's chains
                    // feed the decomposition's
                    let spans = |k: &[f64], d: usize| -> Vec<usize> {
                        (d..k.len() - d - 1).filter(|&i| k[i] < k[i + 1]).collect()
                    };
                    let (su, sv) = (spans(&u, p), spans(&v, q));
                    let inside = |k: &[f64], i: usize, f: f64| k[i] + f * (k[i + 1] - k[i]);
                    let mut from_cut = 0;
                    for a in 0..su.len().max(sv.len()) {
                        for b in a..su.len().max(sv.len()) {
                            let pick = |s: &[usize], i: usize| s[i.min(s.len() - 1)];
                            let (ul, ur) = (
                                inside(&u, pick(&su, a), 0.37),
                                inside(&u, pick(&su, b), 0.61),
                            );
                            let (vl, vr) = (
                                inside(&v, pick(&sv, a), 0.41),
                                inside(&v, pick(&sv, b), 0.58),
                            );
                            let rect = crate::nurbs::geom::Rectangle {
                                ul,
                                ur,
                                vb: vl,
                                vt: vr,
                            };
                            let cut = extract_patch(&source, rect).unwrap();
                            let (_, _, cr, cs) = cut.array_sizes();
                            let aw: Vec<Vec<Tracked>> = (0..cols)
                                .map(|l| {
                                    let col: Vec<Tracked> =
                                        tracked.iter().map(|row| row[l]).collect();
                                    extract_line(&col, p, &u, ul, ur)
                                })
                                .collect();
                            let cut_net: Vec<Vec<Tracked>> = (0..aw[0].len())
                                .map(|i| {
                                    let row: Vec<Tracked> = aw.iter().map(|c| c[i]).collect();
                                    extract_line(&row, q, &v, vl, vr)
                                })
                                .collect();
                            let real: Vec<Vec<CPoint>> =
                                cut.rows().map(<[CPoint]>::to_vec).collect();
                            let mirror: Vec<Vec<CPoint>> = cut_net
                                .iter()
                                .map(|row| row.iter().map(|t| t.0).collect())
                                .collect();
                            assert_eq!(real, mirror, "p {p} q {q} box {a} {b}: the extraction");
                            from_cut = from_cut.max(deepest(
                                &cut_net,
                                p,
                                q,
                                &cut.u_knots()[..=cr],
                                &cut.v_knots()[..=cs],
                                &decompose_to_bez(&cut).unwrap(),
                            ));
                        }
                    }
                    let chain = own.max(from_cut);
                    worst = worst.max(chain);
                    assert!(
                        chain <= insertion_levels(p, q),
                        "p {p} q {q}: {chain} levels, {} counted",
                        insertion_levels(p, q)
                    );
                    assert!(
                        rounding_index(p, q) >= 8 * chain + 9,
                        "p {p} q {q}: γ index {} below 8 · {chain} + 9",
                        rounding_index(p, q)
                    );
                    // (simple knots: the decomposition alone reaches its count)
                    if mu.iter().all(|&m| m == 1) && mv.iter().all(|&m| m == 1) {
                        assert_eq!(own, p * (p - 1) / 2 + q * (q - 1) / 2, "p {p} q {q}");
                    }
                }
            }
        }
        assert_eq!(worst, 96, "the deepest chain, at (10, 10)");
    }
}
