//! Whether a surface is a plane, cylinder, cone, sphere or torus within a tolerance, and which
//! one: a query for the exact algorithms (analytic intersections, blends, offsets), so that a
//! face held as a NURBS (as many STEP files and older designs hold every face) takes them like
//! a face held as its analytic surface. The surface is never changed.
//!
//! A surface of one of those kinds answers with its own numbers. A NURBS form is sampled on a
//! grid of points and unit normals, and the candidates are fitted to the samples:
//!
//! - a plane: every normal one direction, every point on the plane through one of them;
//! - a sphere: the point nearest every normal line in the least-squares sense, the mean
//!   distance to it;
//! - a cylinder, cone or torus: these are surfaces of revolution, whose normal lines all meet
//!   the axis. In Plücker coordinates a line `(d, m)` meets the axis `(a, b)` exactly when
//!   `a . m + d . b = 0`, which is linear in the axis, so the axis is the eigenvector of the
//!   least eigenvalue of the normals' 6 x 6 moment matrix (H. Pottmann and T. Randrup,
//!   "Rotational and helical surface approximation for reverse engineering", Computing 60,
//!   1998). Each sample is then a point `(h, rho)` of the meridian, its height along the axis
//!   and its distance from it, and the meridian is fitted by least squares: a line parallel
//!   to the axis (a cylinder), any line (a cone), a circle (a torus).
//!
//! The first candidate, in that order, that holds every sample within the screening tolerance
//! names the kind. Where the form has that kind's standard structure (rational quadratic
//! circles), [`AnalyticSupport::derive`] reads the exact numbers and frame off its control
//! points; otherwise the fitted numbers stand. Either is then proved on the form: [`lies_on`]
//! (the form is the shape to the rounding of its stored points, `exact`), or, failing that,
//! every point of a grid of at least four samples per control point and direction within the
//! caller's tolerance of it.
//!
//! Upstream: none (not upstream).
//!
//! | Rust                 | Upstream |
//! |----------------------|----------|
//! | [`analytic_of`]      | (none)   |
//! | [`analytic_of_form`] | (none)   |
//! | [`Recognised`]       | (none)   |
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::error::{Result, SolidError};
use crate::geom::matrix::Matrix;
use crate::kernel::Kernel;
use crate::math::{Point2, Vec3};
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::nurbs::knots::Side;
use crate::surface::implicit::{lies_on, plane_of_net, AnalyticSupport, Implicit, SupportKind};
use crate::surface::surface::Surface;

/// A recognised analytic surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recognised {
    /// The surface's implicit form: its kind and numbers.
    pub implicit: Implicit,
    /// Whether the surface is that analytic surface to the rounding of its stored points (an
    /// analytic surface, or a NURBS form proved by [`lies_on`]); otherwise it is within the
    /// tolerance asked for at every sample checked.
    pub exact: bool,
    /// The analytic support with its frame, where the surface has one: an analytic surface's
    /// own, or the one [`AnalyticSupport::derive`] reads off a NURBS form of the standard
    /// structure (proved by [`lies_on`]). [`AnalyticSupport::build`] makes it the analytic
    /// surface on the form's own parametrisation. `implicit` is then this support's.
    pub support: Option<AnalyticSupport>,
}

/// The analytic kind of an implicit surface.
fn kind_of(a: &Implicit) -> SupportKind {
    match a {
        Implicit::Plane { .. } => SupportKind::Plane,
        Implicit::Cone { tan_angle, .. } if *tan_angle == 0.0 => SupportKind::Cylinder,
        Implicit::Cone { .. } => SupportKind::Cone,
        Implicit::Sphere { .. } => SupportKind::Sphere,
        Implicit::Torus { .. } => SupportKind::Torus,
    }
}

/// The kind `kind` read off `form`'s structure and proved on it ([`lies_on`]): the support
/// [`AnalyticSupport::derive`] reads (a plane's affine net, a revolution's rational quadratic
/// circles), or for a plane whose net is not its affine parametrisation, the plane of its net
/// ([`plane_of_net`]). `None` where the form has no such structure.
///
/// # Errors
///
/// [`SolidError::Unported`] and cancellation from the derivation; those of [`lies_on`].
fn structural(
    k: &Kernel,
    kind: SupportKind,
    form: &BSplineSurface,
) -> Result<Option<(Implicit, Option<AnalyticSupport>)>> {
    match AnalyticSupport::derive(k, kind, form) {
        Ok(support) => {
            if lies_on(form, &support.implicit())? {
                return Ok(Some((support.implicit(), Some(support))));
            }
        }
        Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => return Err(e),
        Err(_) => {}
    }
    if kind == SupportKind::Plane {
        return Ok(plane_of_net(form)?.map(|a| (a, None)));
    }
    Ok(None)
}

/// The sample steps per direction the candidates are fitted to (a 13 x 13 grid).
const GRID: usize = 12;

/// The smallest screening tolerance, relative to the form's size: the samples of an exact form
/// lie on its shape to the rounding of evaluating the form (a few hundred ulps of its size),
/// and the fitted numbers are that close, far inside this.
const SCREEN_REL: f64 = 1e-9;

/// The analytic surface `s` is within `tol`: its own kind and numbers when it is a plane,
/// cylinder, cone, sphere or torus, else the one its NURBS form is ([`analytic_of_form`]).
/// `None` for a surface of no such shape, or with no NURBS form.
///
/// # Errors
///
/// Those of making the form and of evaluating it.
pub fn analytic_of(k: &Kernel, s: &Surface, tol: f64) -> Result<Option<Recognised>> {
    if let Some(a) = AnalyticSupport::of(s) {
        return Ok(Some(Recognised {
            implicit: a.implicit(),
            exact: true,
            support: Some(a),
        }));
    }
    match crate::blend::fillet_section::station::as_b_spline(k, s)? {
        Some(form) => analytic_of_form(k, &form, tol),
        None => Ok(None),
    }
}

/// The plane, cylinder, cone, sphere or torus the NURBS `form` is, proved on it: exactly
/// ([`lies_on`]) or with every sample of a dense grid within `tol`. `None` where no candidate
/// fits the samples or none is proved.
///
/// # Errors
///
/// Those of evaluating the form and of [`lies_on`].
pub fn analytic_of_form(k: &Kernel, form: &BSplineSurface, tol: f64) -> Result<Option<Recognised>> {
    let Some(samples) = Samples::of(k, form)? else {
        return Ok(None);
    };
    let screen = tol.max(SCREEN_REL * samples.size);
    let Some(implicit) = samples.fit(screen)? else {
        return Ok(None);
    };
    // the kind's own numbers where the form has its standard structure: read off its control
    // circles, they are exact where a least-squares fit is only close
    if let Some((implicit, support)) = structural(k, kind_of(&implicit), form)? {
        return Ok(Some(Recognised {
            implicit,
            exact: true,
            support,
        }));
    }
    if lies_on(form, &implicit)? {
        return Ok(Some(Recognised {
            implicit,
            exact: true,
            support: None,
        }));
    }
    if tol > 0.0 && within(form, &implicit, tol)? {
        return Ok(Some(Recognised {
            implicit,
            exact: false,
            support: None,
        }));
    }
    Ok(None)
}

/// Whether every point of a grid of at least four samples per control point (and [`GRID`])
/// in each direction of `form` is within `tol` of `a`.
fn within(form: &BSplineSurface, a: &Implicit, tol: f64) -> Result<bool> {
    let steps = |dir| (4 * form.number_control_points(dir)).max(GRID);
    let (nu, nv) = (steps(SurfParamType::U), steps(SurfParamType::V));
    let d = form.natural_uv_domain();
    for i in 0..=nu {
        for j in 0..=nv {
            let p = form.evaluate_point(d.evaluate(ratio(i, nu), ratio(j, nv)))?;
            if !at_most(a.distance(p), tol) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Whether `x <= tol` (false for NaN).
fn at_most(x: f64, tol: f64) -> bool {
    matches!(
        x.partial_cmp(&tol),
        Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
    )
}

/// `i / n` as a double.
fn ratio(i: usize, n: usize) -> f64 {
    i as f64 / n as f64
}

/// A form's samples: its points, the unit normals where it has one (not at a pole or apex,
/// where a first derivative vanishes and the cross product is rounding), the points' centroid
/// and the diagonal of their box.
struct Samples {
    points: Vec<Vec3>,
    normals: Vec<(Vec3, Vec3)>,
    centroid: Vec3,
    size: f64,
}

impl Samples {
    /// The samples of `form` on a `(GRID + 1)^2` grid; `None` for a form that spans no area
    /// (its box a point, or fewer than six normals).
    fn of(k: &Kernel, form: &BSplineSurface) -> Result<Option<Self>> {
        let d = form.natural_uv_domain();
        let mut points = Vec::new();
        let mut normals = Vec::new();
        for i in 0..=GRID {
            for j in 0..=GRID {
                let uv: Point2 = d.evaluate(ratio(i, GRID), ratio(j, GRID));
                let (p, su, sv) = crate::surface::surface::p1::evaluate_1st_derivatives(
                    k,
                    form,
                    uv,
                    Side::Left,
                    Side::Left,
                )?;
                points.push(p);
                // a normal only where the tangents span an area well above their rounding
                let c = su.cross(sv);
                let spans = c.length() > f64::EPSILON.sqrt() * su.length() * sv.length();
                if let (true, Ok(n)) = (spans, c.unitize()) {
                    normals.push((p, n));
                }
            }
        }
        let lo = points
            .iter()
            .fold(Vec3::new(f64::MAX, f64::MAX, f64::MAX), |m, p| {
                Vec3::new(m.x.min(p.x), m.y.min(p.y), m.z.min(p.z))
            });
        let hi = points
            .iter()
            .fold(Vec3::new(f64::MIN, f64::MIN, f64::MIN), |m, p| {
                Vec3::new(m.x.max(p.x), m.y.max(p.y), m.z.max(p.z))
            });
        let size = (hi - lo).length();
        if !(size.is_finite() && size > 0.0) || normals.len() < 6 {
            return Ok(None);
        }
        Ok(Some(Self {
            points,
            normals,
            centroid: (lo + hi) * 0.5,
            size,
        }))
    }

    /// Whether every sample point is within `tol` of `a`.
    fn holds(&self, a: &Implicit, tol: f64) -> bool {
        self.points.iter().all(|p| at_most(a.distance(*p), tol))
    }

    /// The first of a plane, sphere, cylinder, cone or torus that holds every sample within
    /// `tol`.
    fn fit(&self, tol: f64) -> Result<Option<Implicit>> {
        let fits = |a: Implicit| self.holds(&a, tol).then_some(a);
        if let Some(a) = self.plane().and_then(fits) {
            return Ok(Some(a));
        }
        if let Some(a) = self.sphere()?.and_then(fits) {
            return Ok(Some(a));
        }
        let Some((origin, axis)) = self.axis()? else {
            return Ok(None);
        };
        for candidate in self.revolved(origin, axis)? {
            if let Some(a) = fits(candidate) {
                return Ok(Some(a));
            }
        }
        Ok(None)
    }

    /// The plane through the middle normal's point, square to it.
    fn plane(&self) -> Option<Implicit> {
        let &(origin, normal) = self.normals.get(self.normals.len() / 2)?;
        Some(Implicit::Plane { origin, normal })
    }

    /// The sphere about the point nearest every normal line (least squares: the sum over the
    /// normals of `I - n n^T` applied to the point equals it applied to the sample), of the
    /// mean distance from it.
    fn sphere(&self) -> Result<Option<Implicit>> {
        let mut m = [0.0_f64; 9];
        let mut rhs = [0.0_f64; 3];
        for &(p, n) in &self.normals {
            let q = (p - self.centroid) * (1.0 / self.size);
            let (nv, qv) = ([n.x, n.y, n.z], [q.x, q.y, q.z]);
            for r in 0..3 {
                for c in 0..3 {
                    let eye = if r == c { 1.0 } else { 0.0 };
                    let e = eye - at(&nv, r) * at(&nv, c);
                    if let Some(x) = m.get_mut(r * 3 + c) {
                        *x += e;
                    }
                    if let Some(x) = rhs.get_mut(r) {
                        *x += e * at(&qv, c);
                    }
                }
            }
        }
        let Some(x) = solve(3, &m, &rhs)? else {
            return Ok(None);
        };
        let centre = self.centroid + Vec3::new(at(&x, 0), at(&x, 1), at(&x, 2)) * self.size;
        let radius = mean(self.points.iter().map(|p| (*p - centre).length()));
        Ok((radius.is_finite() && radius > 0.0).then_some(Implicit::Sphere { centre, radius }))
    }

    /// The axis every normal line meets, least squares (the eigenvector of the normals' line
    /// complex of least eigenvalue): a point of it and its unit direction.
    fn axis(&self) -> Result<Option<(Vec3, Vec3)>> {
        let mut m = [0.0_f64; 36];
        for &(p, n) in &self.normals {
            let q = (p - self.centroid) * (1.0 / self.size);
            let moment = q.cross(n);
            let row = [moment.x, moment.y, moment.z, n.x, n.y, n.z];
            for (r, &x) in row.iter().enumerate() {
                for (c, &y) in row.iter().enumerate() {
                    if let Some(e) = m.get_mut(r * 6 + c) {
                        *e += x * y;
                    }
                }
            }
        }
        let Ok(eig) = Matrix::from_row_major(6, 6, m.to_vec())?.eigen_system_jacobi() else {
            return Ok(None);
        };
        // the eigenvalues ascend: column 0 is the least's
        let v = |r| eig.vectors.at(r, 0);
        let a = Vec3::new(v(0)?, v(1)?, v(2)?);
        let b = Vec3::new(v(3)?, v(4)?, v(5)?);
        let Some((len, axis)) = a.length_and_unit() else {
            return Ok(None);
        };
        // the axis's point nearest the centroid, `a x b / |a|^2`, back in place
        let near = axis.cross(b * (1.0 / len));
        Ok(Some((self.centroid + near * self.size, axis)))
    }

    /// The cylinder, cone and torus about the axis through `origin` along the unit `axis`
    /// that fit the samples' meridian points `(h, rho)` best, in that order.
    fn revolved(&self, origin: Vec3, axis: Vec3) -> Result<Vec<Implicit>> {
        let meridian: Vec<(f64, f64)> = self
            .points
            .iter()
            .map(|p| {
                let d = *p - origin;
                let h = d.dot(axis);
                (h, (d - axis * h).length())
            })
            .collect();
        let mut out = Vec::new();
        // a cylinder: rho constant
        let radius = mean(meridian.iter().map(|m| m.1));
        out.push(Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle: 0.0,
        });
        // a cone: rho = r + t h, heights and radii scaled by the size
        let s = 1.0 / self.size;
        let mut m = [0.0_f64; 4];
        let mut rhs = [0.0_f64; 2];
        for &(h, rho) in &meridian {
            let row = [1.0, h * s];
            for r in 0..2 {
                for c in 0..2 {
                    if let Some(x) = m.get_mut(r * 2 + c) {
                        *x += at(&row, r) * at(&row, c);
                    }
                }
                if let Some(x) = rhs.get_mut(r) {
                    *x += at(&row, r) * rho * s;
                }
            }
        }
        if let Some(x) = solve(2, &m, &rhs)? {
            out.push(Implicit::Cone {
                origin,
                axis,
                radius: at(&x, 0) * self.size,
                tan_angle: at(&x, 1),
            });
        }
        // a torus: the meridian circle (h - h0)^2 + (rho - R)^2 = r^2, linear as
        // h^2 + rho^2 = 2 h0 h + 2 R rho + c
        let mut m = [0.0_f64; 9];
        let mut rhs = [0.0_f64; 3];
        for &(h, rho) in &meridian {
            let (h, rho) = (h * s, rho * s);
            let row = [2.0 * h, 2.0 * rho, 1.0];
            for r in 0..3 {
                for c in 0..3 {
                    if let Some(x) = m.get_mut(r * 3 + c) {
                        *x += at(&row, r) * at(&row, c);
                    }
                }
                if let Some(x) = rhs.get_mut(r) {
                    *x += at(&row, r) * (h * h + rho * rho);
                }
            }
        }
        if let Some(x) = solve(3, &m, &rhs)? {
            let (h0, major, c) = (at(&x, 0), at(&x, 1), at(&x, 2));
            let minor = (c + h0 * h0 + major * major).sqrt();
            if major > 0.0 && minor.is_finite() && minor > 0.0 {
                out.push(Implicit::Torus {
                    centre: origin + axis * (h0 * self.size),
                    axis,
                    major: major * self.size,
                    minor: minor * self.size,
                });
            }
        }
        Ok(out)
    }
}

/// `v[i]`, or NaN past its end (which no fit accepts).
fn at(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(f64::NAN)
}

/// The mean of `xs`; NaN for none.
fn mean(xs: impl Iterator<Item = f64>) -> f64 {
    let (sum, n) = xs.fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
    sum / ratio(n, 1)
}

/// The solution of the `n x n` normal equations `m x = rhs`; `None` where they are singular.
fn solve(n: usize, m: &[f64], rhs: &[f64]) -> Result<Option<Vec<f64>>> {
    let s = Matrix::from_row_major(n, n, m.to_vec())?;
    Ok(match s.solve_linear_system(rhs) {
        Ok(sol) if !sol.near_singular && sol.x.iter().all(|x| x.is_finite()) => Some(sol.x),
        _ => None,
    })
}
