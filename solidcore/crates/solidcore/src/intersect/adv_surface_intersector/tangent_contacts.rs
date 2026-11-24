//! Search for isolated tangential contact points inside both surfaces
//! (WR-74, tangency-labs/solidcore#394).
//!
//! Not upstream: the SolidCraft fork's change to `SmAdvSurfaceIntersector.cpp`
//! (tangency-labs/smlib@c26edbc). `FindInteriorCurves` only traces crossing curves:
//! a leaf pair whose normals are within 5 degrees at the seed, whose local
//! intersection finds nothing, or whose normals are within 2 degrees at the solution
//! is dropped, so a point where the surfaces merely touch inside both patches is
//! never reported. Here those pairs seed a Newton solve for a tangential
//! contact (Yang, Jia & Yan 2023: zeros of `S1 - S2` with parallel normals):
//!
//! ```text
//! F1 = (Sa - Sb).Sb_s      F2 = (Sa - Sb).Sb_t       (Sb(s,t) the foot of Sa(u,v))
//! F3 = Sa_u.(Sb_s x Sb_t)  F4 = Sa_v.(Sb_s x Sb_t)   (normals parallel)
//! ```
//!
//! with the exact Jacobian, from five seeds per pair and both surface roles, kept to
//! the seed patches. A root is a degenerate `TOUCHING` curve (3D and both UV) only
//! when the gap is numerically zero (`1e-7` of the approximation tolerance), the
//! normals agree to `1e-4` rad and the point is not on a curve found already. Poles
//! (collapsed domain edges), where the system degenerates, are tested directly.
//!
//! | Rust                              | SolidCraft fork (`SmAdvSurfaceIntersector.cpp`) |
//! |-----------------------------------|--------------------------------------------------|
//! | [`find_tangent_contact_points`]   | `sm_FindTangentContactPoints` (static)           |
//! | `TpEval`, `tp_evaluate` (private) | `SmTpEval`, `tp_Evaluate`                        |
//! | `tp_solve4` (private)             | `tp_Solve4`                                      |
//! | `tp_newton` (private)             | `tp_Newton`                                      |
//!
//! Deviations: out-params become return values; `std::min`/`std::max` keep their
//! operand order (`cpp_min`/`cpp_max`); the `SetMinMax` status SMLib ignores is passed
//! on (WR-658, fork e7ebc8d).
//! Not reached for higher-order contacts (equal curvature at the point), where the
//! system is singular.

#[path = "contact_identity.rs"]
mod identity;

use crate::blend::fillet_section::series::{GnScalar, V3};
use crate::curve::curve::p6::DropPointCaches;
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Point3;
use crate::intersect::surface_intersector::p1::{
    surface_evaluate_normal, SrfIndex, SurfaceIntersector,
};
use crate::kernel::Kernel;
use crate::math::{BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, PI};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::surface::p2::TsectCurveType;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;

use super::{InteriorCurves, NoSurfaceHandle};

const OP: &str = "intersect::adv_surface_intersector::find_tangent_contact_points";

/// `std::min(a, b)`: `(b < a) ? b : a`.
fn cpp_min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// `std::max(a, b)`: `(a < b) ? b : a`.
fn cpp_max(a: f64, b: f64) -> f64 {
    if a < b {
        b
    } else {
        a
    }
}

/// `tp_dot`.
fn tp_dot<T: GnScalar>(a: V3<T>, b: V3<T>) -> T {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// `tp_cross`.
fn tp_cross<T: GnScalar>(a: V3<T>, b: V3<T>) -> V3<T> {
    V3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

/// `tp_add`: component sums (as the fork writes them out).
fn tp_add<T: GnScalar>(a: V3<T>, b: V3<T>) -> V3<T> {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

/// `tp_sub`.
fn tp_sub<T: GnScalar>(a: V3<T>, b: V3<T>) -> V3<T> {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// `SmTpEval`: the point and its first and second partials.
#[derive(Debug, Clone, Copy)]
struct TpEval<T = f64> {
    p: V3<T>,
    pu: V3<T>,
    pv: V3<T>,
    puu: V3<T>,
    puv: V3<T>,
    pvv: V3<T>,
}

/// `tp_Evaluate`: `Evaluate(uv, 2, 2, TRUE, TRUE, TRUE, aD, FALSE)`.
fn tp_evaluate(k: &Kernel, srf: &Surface, uv: Point2) -> Result<TpEval> {
    let d = srf.evaluate(
        k,
        uv,
        2,
        2,
        EvaluateFlags {
            u_side: Side::Left,
            v_side: Side::Left,
            only_upper_half: true,
            non_zero_tangents: false,
        },
    )?;
    let at = |i: usize, j: usize| -> Result<V3<f64>> {
        d.get(i)
            .and_then(|row| row.get(j))
            .copied()
            .map(V3::from_vec3)
            .ok_or(SolidError::invalid_input(OP, "short evaluation"))
    };
    Ok(TpEval {
        p: at(0, 0)?,
        pu: at(1, 0)?,
        pv: at(0, 1)?,
        puu: at(2, 0)?,
        puv: at(1, 1)?,
        pvv: at(0, 2)?,
    })
}

/// The stationary parallel-normal system and its Jacobian, in the search's surface
/// order. Floating-point Newton and the rounded interval certificate use the same
/// formulas and operation order. This certifies identity of stationary solutions;
/// the search separately validates each candidate's gap and normals.
fn stationary_system<T: GnScalar>(a: TpEval<T>, b: TpEval<T>) -> ([T; 4], [[T; 4]; 4]) {
    let d = tp_sub(a.p, b.p);
    let m = tp_cross(b.pu, b.pv);
    let ms = tp_add(tp_cross(b.puu, b.pv), tp_cross(b.pu, b.puv));
    let mt = tp_add(tp_cross(b.puv, b.pv), tp_cross(b.pu, b.pvv));
    let neg = |x: T| T::of(-1.0) * x;
    let f = [
        tp_dot(d, b.pu),
        tp_dot(d, b.pv),
        tp_dot(a.pu, m),
        tp_dot(a.pv, m),
    ];
    let j = [
        [
            tp_dot(a.pu, b.pu),
            tp_dot(a.pv, b.pu),
            neg(tp_dot(b.pu, b.pu)) + tp_dot(d, b.puu),
            neg(tp_dot(b.pv, b.pu)) + tp_dot(d, b.puv),
        ],
        [
            tp_dot(a.pu, b.pv),
            tp_dot(a.pv, b.pv),
            neg(tp_dot(b.pu, b.pv)) + tp_dot(d, b.puv),
            neg(tp_dot(b.pv, b.pv)) + tp_dot(d, b.pvv),
        ],
        [
            tp_dot(a.puu, m),
            tp_dot(a.puv, m),
            tp_dot(a.pu, ms),
            tp_dot(a.pu, mt),
        ],
        [
            tp_dot(a.puv, m),
            tp_dot(a.pvv, m),
            tp_dot(a.pv, ms),
            tp_dot(a.pv, mt),
        ],
    ];
    (f, j)
}

/// `tp_Solve4`: solves `a x = b` by Gaussian elimination with partial pivoting;
/// `None` when a pivot vanishes or the reduced diagonal is numerically singular.
fn tp_solve4(mut a: [[f64; 4]; 4], mut b: [f64; 4]) -> Option<[f64; 4]> {
    let at = |a: &[[f64; 4]; 4], i: usize, j: usize| a.get(i).and_then(|r| r.get(j)).copied();
    for k in 0..4 {
        let mut i_piv = k;
        for i in k + 1..4 {
            if at(&a, i, k)?.abs() > at(&a, i_piv, k)?.abs() {
                i_piv = i;
            }
        }
        if at(&a, i_piv, k)?.abs() < 1.0e-300 {
            return None;
        }
        if i_piv != k {
            a.swap(k, i_piv);
            b.swap(k, i_piv);
        }
        let row_k = *a.get(k)?;
        let b_k = *b.get(k)?;
        let a_kk = *row_k.get(k)?;
        let (_, rest) = a.split_at_mut(k + 1);
        for (row, b_i) in rest.iter_mut().zip(b.iter_mut().skip(k + 1)) {
            let f = *row.get(k)? / a_kk;
            for (v, a_kj) in row.iter_mut().zip(row_k.iter()).skip(k) {
                *v -= f * a_kj;
            }
            *b_i -= f * b_k;
        }
    }
    let mut x = [0.0; 4];
    for i in (0..4).rev() {
        let row = a.get(i)?;
        let mut s = *b.get(i)?;
        for j in i + 1..4 {
            s -= row.get(j)? * x.get(j)?;
        }
        *x.get_mut(i)? = s / row.get(i)?;
    }
    // reject a numerically singular system (relative pivot test)
    let mut d_max = 0.0;
    let mut d_min = BIG_DOUBLE;
    for (i, row) in a.iter().enumerate() {
        let d = row.get(i)?.abs();
        d_max = cpp_max(d_max, d);
        d_min = cpp_min(d_min, d);
    }
    (d_min > 1.0e-13 * d_max).then_some(x)
}

/// The result of [`tp_newton`]: the parameters when the iteration converged, and the
/// gap `|Sa - Sb|` there.
struct NewtonResult {
    uv_a: Point2,
    uv_b: Point2,
    gap: f64,
}

/// `seed` grown by half its size on each side (`tp_Newton`'s `sLocA`/`sLocB`).
fn grown(seed: &Extent2d) -> Result<Extent2d> {
    let mut loc = *seed;
    let grow = seed.size();
    // (upstream ignores the status and keeps the extent; fork e7ebc8d passes it on: `SER`)
    loc.set_min_max_coords(
        seed.min().x - 0.5 * grow.x,
        seed.min().y - 0.5 * grow.y,
        seed.max().x + 0.5 * grow.x,
        seed.max().y + 0.5 * grow.y,
    )?;
    Ok(loc)
}

/// `tp_Newton`: Newton iteration for a tangential contact of `sa` (u, v) and `sb`
/// (s, t) from `uv_a`, `uv_b`; `None` when it did not converge (`rbConverged`
/// false), was singular, or left the seed patches grown by half their size.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one fork routine
#[allow(clippy::similar_names)]
fn tp_newton(
    k: &Kernel,
    sa: &Surface,
    dom_a: &Extent2d,
    sb: &Surface,
    dom_b: &Extent2d,
    seed_a: &Extent2d,
    seed_b: &Extent2d,
    uv_a: Point2,
    uv_b: Point2,
) -> Result<Option<NewtonResult>> {
    // a seed only has to find a contact near its own patch pair: give up when the
    // iteration leaves the seed patches grown by half their size each side
    let loc_a = grown(seed_a)?;
    let loc_b = grown(seed_b)?;

    let mut converged = false;
    let size_a = dom_a.size();
    let size_b = dom_b.size();
    let max_step = [
        0.25 * size_a.x,
        0.25 * size_a.y,
        0.25 * size_b.x,
        0.25 * size_b.y,
    ];
    let mut par = [uv_a.x, uv_a.y, uv_b.x, uv_b.y];

    // a few Gauss-Newton projection steps first so (s,t) starts at the foot of Sa(u,v)
    for _ in 0..8 {
        let a = tp_evaluate(k, sa, Point2::new(par[0], par[1]))?;
        let b = tp_evaluate(k, sb, Point2::new(par[2], par[3]))?;
        let d = tp_sub(a.p, b.p);
        let a11 = tp_dot(b.pu, b.pu);
        let a12 = tp_dot(b.pu, b.pv);
        let a22 = tp_dot(b.pv, b.pv);
        let r1 = tp_dot(d, b.pu);
        let r2 = tp_dot(d, b.pv);
        let det = a11 * a22 - a12 * a12;
        if det.abs() < 1.0e-300 {
            break;
        }
        let mut ds = (a22 * r1 - a12 * r2) / det;
        let mut dt = (-a12 * r1 + a11 * r2) / det;
        ds = cpp_max(-max_step[2], cpp_min(max_step[2], ds));
        dt = cpp_max(-max_step[3], cpp_min(max_step[3], dt));
        let new = dom_b.clamp_point(Point2::new(par[2] + ds, par[3] + dt));
        let d_move = (new.x - par[2]).abs() / cpp_max(size_b.x, EFF_ZERO)
            + (new.y - par[3]).abs() / cpp_max(size_b.y, EFF_ZERO);
        par[2] = new.x;
        par[3] = new.y;
        if d_move < 1.0e-12 {
            break;
        }
    }

    for _ in 0..25 {
        let a = tp_evaluate(k, sa, Point2::new(par[0], par[1]))?;
        let b = tp_evaluate(k, sb, Point2::new(par[2], par[3]))?;
        let (f, j) = stationary_system(a, b);
        let rhs = [-f[0], -f[1], -f[2], -f[3]];
        let Some(mut dx) = tp_solve4(j, rhs) else {
            return Ok(None);
        };

        let sizes = [size_a.x, size_a.y, size_b.x, size_b.y];
        let mut d_rel = 0.0;
        for ((step, &max), &size) in dx.iter_mut().zip(&max_step).zip(&sizes) {
            *step = cpp_max(-max, cpp_min(max, *step));
            d_rel = cpp_max(d_rel, step.abs() / cpp_max(size, EFF_ZERO));
        }
        let new_a = dom_a.clamp_point(Point2::new(par[0] + dx[0], par[1] + dx[1]));
        let new_b = dom_b.clamp_point(Point2::new(par[2] + dx[2], par[3] + dx[3]));
        par = [new_a.x, new_a.y, new_b.x, new_b.y];
        if d_rel < 1.0e-13 {
            converged = true;
            break;
        }
        if !loc_a.contains_point(new_a, 0.0) || !loc_b.contains_point(new_b, 0.0) {
            return Ok(None);
        }
    }
    if !converged {
        return Ok(None);
    }

    let uv_a = Point2::new(par[0], par[1]);
    let uv_b = Point2::new(par[2], par[3]);
    let pa = sa.evaluate_point(k, uv_a)?;
    let pb = sb.evaluate_point(k, uv_b)?;
    Ok(Some(NewtonResult {
        uv_a,
        uv_b,
        gap: pa.distance(pb),
    }))
}

/// The state of one [`find_tangent_contact_points`] run (the fork's lambdas'
/// captures).
struct Search<'a, 's> {
    si: &'a SurfaceIntersector<'s>,
    /// Only accepted contacts participate in identity comparisons.
    contacts: Vec<(identity::Contact, Point3)>,
    identities: Option<identity::Comparator>,
    /// `lNumCurvesIn`.
    num_curves_in: usize,
    /// `dGapTol`.
    gap_tol: f64,
}

impl Search<'_, '_> {
    /// `GapOk`: a true contact converges to a numerically zero gap.
    fn gap_ok(&self, gap: f64, p: Point3) -> bool {
        let scale = 1.0 + p.x.abs() + p.y.abs() + p.z.abs();
        gap <= cpp_max(self.gap_tol, 1.0e-14 * scale)
    }

    /// `Accept`: adds the candidate `(uv1, uv2)` as a degenerate `TOUCHING` curve
    /// when its gap is numerically zero, it is new, the normals are parallel and it is
    /// not on a curve found already.
    fn accept(
        &mut self,
        k: &Kernel,
        uv1: Point2,
        uv2: Point2,
        gap: f64,
        out: &mut InteriorCurves,
        caches: &mut DropPointCaches,
    ) -> Result<()> {
        let srf1 = self.si.surface(SrfIndex::First);
        let srf2 = self.si.surface(SrfIndex::Second);
        let p1 = srf1.evaluate_point(k, uv1)?;
        let p2 = srf2.evaluate_point(k, uv2)?;
        let pnt = (p1 + p2) / 2.0;
        if !self.gap_ok(gap, pnt) {
            return Ok(());
        }

        // normals must be parallel (EvaluateNormal also handles poles)
        let n1 = surface_evaluate_normal(k, srf1, uv1)?;
        let n2 = surface_evaluate_normal(k, srf2, uv2)?;
        if n1.length_squared() < EFF_ZERO_SQ || n2.length_squared() < EFF_ZERO_SQ {
            return Ok(());
        }
        let mut ang = n1.angle_between(n2)?;
        if ang > PI / 2.0 {
            ang = PI - ang;
        }
        if ang > 1.0e-4 {
            return Ok(());
        }

        // Curves and isolated points have different identity tests. In particular,
        // a point inside another contact's modelling tolerance may be a distinct root.
        let existing = out.curves_3d.get(..self.num_curves_in).ok_or_else(|| {
            SolidError::port_invariant(OP, "the initial intersection curves were removed")
        })?;
        if self
            .si
            .is_point_on_curve_cached(k, pnt, existing, None, caches)?
        {
            return Ok(());
        }
        let reach = 100.0 * self.si.this_approx_tol_3d;

        let mut contact = identity::Contact::new([uv1, uv2]);
        if !self.contacts.is_empty() && self.identities.is_none() {
            self.identities = Some(identity::Comparator::new(
                k,
                [srf1, srf2],
                self.si.uv_domains,
            )?);
        }
        for (old, point) in &mut self.contacts {
            let identity = match &self.identities {
                Some(comparator) => comparator.same(k, [srf1, srf2], old, &mut contact)?,
                None => None,
            };
            if identity.unwrap_or_else(|| point.distance(pnt) < reach) {
                return Ok(());
            }
        }

        out.curves_3d
            .push(Curve::from(BSplineCurve::create_degenerate_curve(3, pnt)?));
        out.surface1_uv_curves
            .push(Curve::from(BSplineCurve::create_degenerate_curve(
                2,
                Point3::new(uv1.x, uv1.y, 0.0),
            )?));
        out.surface2_uv_curves
            .push(Curve::from(BSplineCurve::create_degenerate_curve(
                2,
                Point3::new(uv2.x, uv2.y, 0.0),
            )?));
        out.deviations.push(gap);
        out.curve_types.push(TsectCurveType::Touching);
        self.contacts.push((contact, pnt));
        Ok(())
    }
}

/// The seed fractions of a patch (`sadFr`).
const SEED_FRACTIONS: [[f64; 2]; 5] = [[0.5, 0.5], [0.2, 0.2], [0.8, 0.2], [0.2, 0.8], [0.8, 0.8]];

/// `sm_FindTangentContactPoints`: finds the isolated tangential contact points inside
/// both surfaces of `si` (over its UV domains), seeded by the leaf pairs `seeds`
/// (their patch UV domains) that `FindInteriorCurves` could not trace, and appends
/// each one to `out` as a degenerate [`TsectCurveType::Touching`] curve. `caches`
/// are the drop caches of `out.curves_3d` (slot `i` is curve `i`'s).
///
/// # Errors
///
/// Those of the evaluations, drops and curve creation (the fork's `SER`s).
pub(super) fn find_tangent_contact_points(
    k: &Kernel,
    si: &SurfaceIntersector<'_>,
    seeds: &[[Extent2d; 2]],
    out: &mut InteriorCurves,
    caches: &mut DropPointCaches,
) -> Result<()> {
    let srf1 = si.surface(SrfIndex::First);
    let srf2 = si.surface(SrfIndex::Second);
    let dom1 = si.uv_domains[0];
    let dom2 = si.uv_domains[1];
    let tol_3d = si.this_approx_tol_3d;

    // A true contact converges to a numerically zero gap. A small nonzero gap at a
    // parallel-normal point means a near miss or a small intersection loop of radius
    // ~sqrt(gap/curvature), which must not be reported as a point; the gap limit
    // keeps such loops below the approximation tolerance.
    let mut search = Search {
        si,
        contacts: Vec::new(),
        identities: None,
        num_curves_in: out.curves_3d.len(),
        gap_tol: 1.0e-7 * tol_3d,
    };

    // Poles: the Newton system degenerates where a surface's partials vanish, so
    // test each collapsed domain edge (pole) of either surface directly: a pole lying
    // on the other surface with parallel normals is a contact point.
    for i_srf in 0..2 {
        let (srf, dom, oth, o_dom) = if i_srf == 0 {
            (srf1, &dom1, srf2, &dom2)
        } else {
            (srf2, &dom2, srf1, &dom1)
        };
        for i_edge in 0..4 {
            let uvs = [1.0, 2.0, 3.0].map(|n: f64| {
                let f = 0.25 * n;
                match i_edge {
                    0 => dom.evaluate(f, 0.0),
                    1 => dom.evaluate(f, 1.0),
                    2 => dom.evaluate(0.0, f),
                    _ => dom.evaluate(1.0, f),
                }
            });
            let [uv0, uv1, uv2] = uvs;
            let p0 = srf.evaluate_point(k, uv0)?;
            let p1 = srf.evaluate_point(k, uv1)?;
            let p2 = srf.evaluate_point(k, uv2)?;
            if !search.gap_ok(p0.distance(p1), p1) || !search.gap_ok(p1.distance(p2), p1) {
                continue; // not a pole
            }
            let drop =
                oth.drop_point::<NoSurfaceHandle>(k, p1, o_dom, &DropPointOptions::default())?;
            if !drop.success {
                continue;
            }
            if i_srf == 0 {
                search.accept(k, uv1, drop.result_uv, drop.gap, out, caches)?;
            } else {
                search.accept(k, drop.result_uv, uv1, drop.gap, out, caches)?;
            }
        }
    }

    for [d1, d2] in seeds {
        for fr in &SEED_FRACTIONS {
            for i_role in 0..2 {
                // seed on one patch, start the other at its patch center
                let found = if i_role == 0 {
                    let uv1 = d1.evaluate(fr[0], fr[1]);
                    let uv2 = d2.evaluate(0.5, 0.5);
                    tp_newton(k, srf1, &dom1, srf2, &dom2, d1, d2, uv1, uv2)?
                        .map(|r| (r.uv_a, r.uv_b, r.gap))
                } else {
                    let uv2 = d2.evaluate(fr[0], fr[1]);
                    let uv1 = d1.evaluate(0.5, 0.5);
                    tp_newton(k, srf2, &dom2, srf1, &dom1, d2, d1, uv2, uv1)?
                        .map(|r| (r.uv_b, r.uv_a, r.gap))
                };
                let Some((uv1, uv2, gap)) = found else {
                    continue;
                };
                search.accept(k, uv1, uv2, gap, out, caches)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec2;
    use crate::surface::plane::Plane;

    #[test]
    fn an_inconclusive_contact_comparison_uses_the_distance_check() {
        let k = Kernel::default();
        let domain = Extent2d::from_bounds(-1.0, -1.0, 1.0, 1.0);
        let surface = Surface::from(Plane::new(
            Point3::ZERO,
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Vec2::new(1.0, 1.0),
            &domain,
        ));
        let mut si =
            SurfaceIntersector::new(&k, &surface, &domain, &surface, &domain, false).unwrap();
        si.this_approx_tol_3d = 1e-6;
        let mut search = Search {
            si: &si,
            contacts: Vec::new(),
            identities: None,
            num_curves_in: 0,
            gap_tol: 0.0,
        };
        let mut out = InteriorCurves::default();
        let mut caches = DropPointCaches::default();
        // Coplanar samples give a singular stationary Jacobian. The distance
        // check merges the first two and keeps the far one.
        for x in [0.0, 5e-5, 0.1] {
            let uv = Point2::new(x, 0.0);
            search
                .accept(&k, uv, uv, 0.0, &mut out, &mut caches)
                .unwrap();
        }
        assert_eq!(out.curve_types, [TsectCurveType::Touching; 2]);
    }

    #[test]
    fn solve4_solves_and_rejects_singular() {
        let a = [
            [4.0, 1.0, 0.0, 0.0],
            [1.0, 3.0, 1.0, 0.0],
            [0.0, 1.0, 2.0, 1.0],
            [0.0, 0.0, 1.0, 5.0],
        ];
        let x = tp_solve4(a, [1.0, 2.0, 3.0, 4.0]).unwrap();
        for (row, b) in a.iter().zip([1.0, 2.0, 3.0, 4.0]) {
            let r: f64 = row.iter().zip(&x).map(|(a, x)| a * x).sum();
            assert!((r - b).abs() < 1e-12);
        }
        let mut s = a;
        s[3] = s[2];
        assert!(tp_solve4(s, [1.0; 4]).is_none());
    }

    #[test]
    fn cpp_min_max_keep_operand_order() {
        assert_eq!(cpp_min(1.0, f64::NAN), 1.0);
        assert_eq!(cpp_max(-1.0, f64::NAN), -1.0);
        assert_eq!(cpp_max(-1.0, cpp_min(1.0, 0.5)), 0.5);
    }
}
