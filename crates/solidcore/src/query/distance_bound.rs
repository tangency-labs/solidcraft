//! Not upstream: a certified lower bound of the distance from a point to a face, by the
//! convex hull property of its NURBS form's Bézier patches (Piegl and Tiller, *The NURBS
//! Book*, 2nd ed., §4.4: with positive weights a rational patch lies in the convex hull of
//! its Euclidean control points), tightened by de Casteljau subdivision (Farouki and Rajan
//! 1987) on the Bernstein algebra of [`crate::offset::construction::bernstein`].
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`face_distance_lower_bound`], [`surface_distance_lower_bound`] | none |
//!
//! The face is bounded by its surface over the face's UV box (its trims lie inside it), so
//! the bound is the distance to a superset of the face: never more than the face's
//! distance. Rounding is carried as a derived absolute error on every homogeneous
//! coefficient (`gamma_k` of Higham, *Accuracy and Stability of Numerical Algorithms*,
//! 2nd ed., §3.1, for the translation to the point and each de Casteljau level), so each
//! patch's box is widened by the error its Euclidean points can carry, and the box's
//! distance is rounded down. No tolerance of its own: the bound is what the arithmetic
//! proves.
//!
//! Deviations: none (no upstream counterpart). No recursion: the patches wait on a heap,
//! at most [`MAX_PATCHES`] of them.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::blend::fillet_section::station::as_b_spline;
use crate::geom::certified::above;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::math::{gamma, NOW, NOZ};
use crate::nurbs::srf_tool::p1::decompose_to_bez;
use crate::offset::construction::bernstein::Bernstein2;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::surface_cache::p3::definition_bound;
use crate::topology::{Brep, FaceId};
use crate::{Result, SolidError};

const OP: &str = "query::distance_bound::face_distance_lower_bound";

/// The most patches subdivided for one bound.
pub const MAX_PATCHES: usize = 4096;

/// A piece of a face's surface: its homogeneous coordinates about the point, as Bernstein
/// polynomials on the unit square, and the absolute error each coefficient may carry.
#[derive(Debug, Clone)]
struct Piece {
    /// `X W - p W`, `Y W - p W`, `Z W - p W`, `W`.
    h: [Bernstein2; 4],
    /// The bound on every coefficient's error.
    err: f64,
    /// The certified lower bound of the piece's distance from the point.
    lower: f64,
    /// Its subdivision depth.
    depth: usize,
}

impl PartialEq for Piece {
    fn eq(&self, o: &Self) -> bool {
        self.lower.total_cmp(&o.lower) == Ordering::Equal
    }
}
impl Eq for Piece {}
impl PartialOrd for Piece {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Piece {
    // the heap pops the smallest bound first
    fn cmp(&self, o: &Self) -> Ordering {
        o.lower.total_cmp(&self.lower)
    }
}

impl Piece {
    /// The piece with its bound, the coefficients `h` within `err` each.
    fn new(h: [Bernstein2; 4], err: f64, depth: usize) -> Result<Self> {
        let lower = hull_bound(&h, err)?;
        Ok(Self {
            h,
            err,
            lower,
            depth,
        })
    }

    /// Its four quarters: one de Casteljau level each way, which adds the rounding of a
    /// convex combination per level (`gamma` of twice the degree plus two) of the largest
    /// coefficient.
    fn quarters(&self) -> Result<[Self; 4]> {
        let largest = self
            .h
            .iter()
            .map(Bernstein2::max_abs_coefficient)
            .fold(0.0_f64, f64::max);
        let (du, dv) = self.h[3].degrees();
        let err = self.err + gamma(2 * (du + dv) + 4) * (largest + self.err);
        let split = |h: &Bernstein2| {
            let (l, r) = h.split_u(0.5);
            let (ll, lr) = l.split_v(0.5);
            let (rl, rr) = r.split_v(0.5);
            [ll, lr, rl, rr]
        };
        let [x, y, z, w] = [
            split(&self.h[0]),
            split(&self.h[1]),
            split(&self.h[2]),
            split(&self.h[3]),
        ];
        let mut out = Vec::new();
        for i in 0..4 {
            let pick =
                |a: &[Bernstein2; 4]| a.get(i).cloned().ok_or_else(|| SolidError::sm_err(OP));
            out.push(Self::new(
                [pick(&x)?, pick(&y)?, pick(&z)?, pick(&w)?],
                err,
                self.depth + 1,
            )?);
        }
        out.try_into().map_err(|_| SolidError::sm_err(OP))
    }
}

/// The certified lower bound of the distance from the origin (the point) to the patch whose
/// homogeneous coefficients `h` carry at most `err` each, from the hull of its Euclidean
/// control points, each widened by the error a quotient of coefficients within `err` can
/// carry: the larger of two bounds, each rounded down. The box's distance; and the hull's
/// support along `u`, the direction of the points' mean (for X in the hull, X . u is at
/// least the least P . u, and |X| >= X . u / |u|), which, unlike the box, tightens with the
/// square of the piece's size on a piece tilted to the axes.
fn hull_bound(h: &[Bernstein2; 4], err: f64) -> Result<f64> {
    let [x, y, z, w] = h;
    let (xs, ys, zs, ws) = (
        x.coefficients(),
        y.coefficients(),
        z.coefficients(),
        w.coefficients(),
    );
    let w_min = ws.iter().copied().fold(f64::INFINITY, f64::min);
    if !above(w_min - err, 0.0) {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "a patch's weights are not proved positive",
        });
    }
    let mut points = Vec::new();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut size = 0.0_f64;
    let mut sum_points = [0.0_f64; 3];
    for (((&a, &b), &c), &wi) in xs.iter().zip(ys).zip(zs).zip(ws) {
        let e = [a / wi, b / wi, c / wi];
        for (k, v) in e.into_iter().enumerate() {
            size = size.max(v.abs());
            if let (Some(l), Some(u), Some(t)) =
                (lo.get_mut(k), hi.get_mut(k), sum_points.get_mut(k))
            {
                *l = l.min(v);
                *u = u.max(v);
                *t += v;
            }
        }
        points.push(e);
    }
    // |(v + dv) / (w + dw) - v / w| <= err (1 + |v / w|) / (w - err), and the quotient's
    // own rounding
    let widen = err * (1.0 + size) / (w_min - err) + gamma(2) * size;
    let mut sum = 0.0_f64;
    for (l, u) in lo.into_iter().zip(hi) {
        let out = (l - widen).max(-(u + widen)).max(0.0);
        // (each difference rounded up by at most gamma_1 of its terms' size)
        let out = (out - gamma(1) * (l.abs().max(u.abs()) + widen)).max(0.0);
        sum += out * out;
    }
    let by_box = sum.sqrt() * (1.0 - gamma(5));
    // the support along u: each P . u rounded within gamma_3 |P| |u|, each P within widen
    // per coordinate of the true point (sqrt 3 widen in length, under 2 widen)
    let [sx, sy, sz] = sum_points;
    let len = (sx * sx + sy * sy + sz * sz).sqrt();
    let mut by_support = 0.0_f64;
    if len > 0.0 && len.is_finite() {
        let u = [sx / len, sy / len, sz / len];
        let u_len = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt() * (1.0 + gamma(6));
        let least = points
            .iter()
            .map(|e| e[0] * u[0] + e[1] * u[1] + e[2] * u[2])
            .fold(f64::INFINITY, f64::min);
        let slack = 2.0 * widen * u_len + gamma(3) * size * 3.0 * u_len;
        let lower = (least - slack) / u_len * (1.0 - gamma(2));
        if lower.is_finite() {
            by_support = lower.max(0.0);
        }
    }
    let bound = by_box.max(by_support);
    if !bound.is_finite() {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "a patch's box is not finite",
        });
    }
    Ok(bound.max(0.0))
}

/// A certified lower bound of the distance from `p` to face `f` of `b`: no point of the face
/// is nearer to `p`. The face is bounded by its surface over the face's UV box (its trims
/// lie inside it): [`surface_distance_lower_bound`] on them.
///
/// # Errors
///
/// `InvalidInput` for a face without a surface; those of [`surface_distance_lower_bound`].
pub fn face_distance_lower_bound(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    p: Point3,
    want: f64,
) -> Result<f64> {
    let face = b.face(f)?;
    let surf = face
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "a face without a surface"))?;
    surface_distance_lower_bound(k, surf, Some(&face.uv_domain), p, want)
}

/// The far point of an analytic surface's definition its NURB's constants are computed about:
/// the placement origin of a plane, cone, sphere or torus, the axis point of a revolution, the
/// origin of an extrusion; `None` for a surface that evaluates on its net (a B-spline, a
/// patch, a blend's net).
fn analytic_origin(k: &Kernel, surf: &Surface) -> Result<Option<Point3>> {
    use crate::surface::implicit::AnalyticSupport as A;
    Ok(match surf.kind_with_nurb(k)? {
        SurfaceKind::Plane(p) => Some(p.position().origin()),
        SurfaceKind::Cone(c) => Some(c.canonical().origin.origin()),
        SurfaceKind::Sphere(_) | SurfaceKind::Torus(_) => match A::of(surf) {
            Some(A::Sphere { placement, .. } | A::Torus { placement, .. }) => {
                Some(placement.origin())
            }
            // (a sphere or torus without its analytic data: refused by the caller)
            _ => None,
        },
        SurfaceKind::SurfOfRevolution(r) => Some(r.position.origin()),
        SurfaceKind::SurfOfExtrusion(e) => Some(e.origin),
        _ => None,
    })
}

/// Whether [`surface_distance_lower_bound`] can bound the distance to `surf`: it has a NURBS
/// form, three-dimensional, every coefficient finite, every weight positive (the hull holds
/// with positive weights, and subdivision keeps them so). The one test the bound and the
/// callers that must refuse before a search ([`crate::offset::wall_room`]) share.
///
/// # Errors
///
/// Those of making the NURB.
pub fn measurable(k: &Kernel, surf: &Surface) -> Result<bool> {
    let Some(form) = as_b_spline(k, surf)? else {
        return Ok(false);
    };
    let Some(nurb) = form.gw_nurb() else {
        return Ok(false);
    };
    for row in nurb.rows() {
        for q in row {
            let w = if q.w == NOW { 1.0 } else { q.w };
            let finite = q.x.is_finite() && q.y.is_finite() && q.z.is_finite() && w.is_finite();
            if q.z == NOZ || !finite || !above(w, 0.0) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// A certified lower bound of the distance from `p` to `surf` over `uv_box` (its NURB
/// parameters; `None`, or an empty box, for the whole surface): no point of it is nearer to
/// `p`. The NURBS form's Bézier patches over the box are subdivided, nearest bound first,
/// until the nearest is at least `want` (then so is the bound returned), the nearest piece's
/// centre is nearer than `want`, or [`MAX_PATCHES`] pieces have been made; the bound returned
/// is the least over the pieces left, so it is certified whatever stopped the subdivision.
///
/// What the bound carries:
/// - a surface that evaluates on a definition its net is within a certified bound of (a
///   procedural blend, `sm_DefinitionBound`): that bound subtracted, clamped at 0;
/// - the Bézier extraction's knot insertions, when the net has interior knots below full
///   multiplicity: each insertion's `alpha = (u_b - u_a) / (u_k - u_a)` within `gamma_3`,
///   `1 - alpha` one more rounding and `alpha P + (1 - alpha) Q` three, at most `8 u M` with
///   `|P|, |Q| <= M`; a coefficient passes through at most `p - 1` insertions at each end of
///   its span in u and `q - 1` in v: `gamma_{16 (p + q)}` of the net's largest homogeneous
///   coefficient (`decompose_to_bez`, The NURBS Book A5.6);
/// - an analytic surface's form against its definition (the face evaluates on the analytic
///   surface): its constants are the definition's data through at most eight roundings per
///   coordinate (the circle weights' `cos`/`sin` an ulp each, two roundings, as
///   [`crate::geom::certified`] counts them; the products and sums with the frame), on sizes
///   at most the net's plus the distance of the definition's origin, and a generator's own
///   circle within twice the net's size: `gamma_32 (2 M + |O|)`;
/// - the translation to `p`, the restriction to the box (four de Casteljau levels, measured on
///   the coefficients before it) and each subdivision ([`Piece`]).
///
/// # Errors
///
/// `InvalidInput` for a point or `want` not finite, `want` negative, or a UV box not inside
/// the patches' knot range (to its rounding); `Unsupported` for a surface without a NURBS
/// form, a 2D form, or weights not proved positive; those of making the form and of its
/// Bézier decomposition.
pub fn surface_distance_lower_bound(
    k: &Kernel,
    surf: &Surface,
    uv_box: Option<&Extent2d>,
    p: Point3,
    want: f64,
) -> Result<f64> {
    if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) {
        return Err(SolidError::invalid_input(OP, "point not finite"));
    }
    if !(want.is_finite() && want >= 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the bound asked for is finite and not negative",
        ));
    }
    if !measurable(k, surf)? {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "a surface without a finite 3D NURBS form of positive weights",
        });
    }
    let unsupported = SolidError::Unsupported {
        operation: OP,
        reason: "a surface without a NURBS form",
    };
    let form = as_b_spline(k, surf)?.ok_or_else(|| unsupported.clone())?;
    let nurb = form.gw_nurb().ok_or(unsupported)?;
    let slack = definition_bound(surf);
    // the net's largest homogeneous coefficient, for the extraction's and the form's rounding
    let mut net_size = 0.0_f64;
    for row in nurb.rows() {
        for q in row {
            let w = if q.w == NOW { 1.0 } else { q.w };
            net_size = net_size
                .max(q.x.abs())
                .max(q.y.abs())
                .max(q.z.abs())
                .max(w.abs());
        }
    }
    let (np, nq) = (nurb.p(), nurb.q());
    // an interior knot below full multiplicity is where the extraction inserts
    let inserts = |knots: &[f64], degree: usize| {
        let (Some(&first), Some(&last)) = (knots.first(), knots.last()) else {
            return false;
        };
        let mut i = 0;
        while let Some(&u) = knots.get(i) {
            let mut m = 1;
            while knots.get(i + m) == Some(&u) {
                m += 1;
            }
            if u > first && u < last && m < degree {
                return true;
            }
            i += m;
        }
        false
    };
    let mut form_err = 0.0;
    if inserts(nurb.u_knots(), np) || inserts(nurb.v_knots(), nq) {
        form_err += gamma(16 * (np + nq)) * net_size;
    }
    let kind = surf.kind_with_nurb(k)?;
    let analytic_kind = matches!(
        kind,
        SurfaceKind::Plane(_)
            | SurfaceKind::Cone(_)
            | SurfaceKind::Sphere(_)
            | SurfaceKind::Torus(_)
            | SurfaceKind::SurfOfRevolution(_)
            | SurfaceKind::SurfOfExtrusion(_)
    );
    let origin = analytic_origin(k, surf)?;
    if analytic_kind && origin.is_none() {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "an analytic surface without its analytic data",
        });
    }
    if let Some(o) = origin {
        let far = o.x.abs().max(o.y.abs()).max(o.z.abs());
        form_err += gamma(32) * (2.0 * net_size + far);
    }
    let patches = decompose_to_bez(nurb)?;
    // the patches' knot range
    let (mut ulo, mut uhi, mut vlo, mut vhi) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for patch in patches.iter().flatten() {
        let (uk, vk) = (patch.u_knots(), patch.v_knots());
        for &u in uk {
            ulo = ulo.min(u);
            uhi = uhi.max(u);
        }
        for &v in vk {
            vlo = vlo.min(v);
            vhi = vhi.max(v);
        }
    }
    let dom = uv_box.filter(|d| {
        let (lo, hi) = (d.min(), d.max());
        lo.x < hi.x && lo.y < hi.y
    });
    if let Some(d) = dom {
        let (lo, hi) = (d.min(), d.max());
        // the box's ends within the rounding of the knots they were made from
        let du = 8.0 * f64::EPSILON * (ulo.abs() + uhi.abs());
        let dv = 8.0 * f64::EPSILON * (vlo.abs() + vhi.abs());
        if !(lo.x >= ulo - du && hi.x <= uhi + du && lo.y >= vlo - dv && hi.y <= vhi + dv) {
            return Err(SolidError::invalid_input(
                OP,
                "a UV box outside its surface's patches",
            ));
        }
    }
    let want_net = want + slack;
    let mut heap = BinaryHeap::new();
    let mut made = 0_usize;
    for patch in patches.iter().flatten() {
        let (pu, pv) = patch.degrees();
        let (uk, vk) = (patch.u_knots(), patch.v_knots());
        let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) =
            (uk.first(), uk.last(), vk.first(), vk.last())
        else {
            return Err(SolidError::sm_err(OP));
        };
        // the part of the patch inside the UV box, in the patch's [0, 1]^2
        let (mut a0, mut a1, mut b0, mut b1) = (0.0, 1.0, 0.0, 1.0);
        if let Some(d) = dom {
            let (dlo, dhi) = (d.min(), d.max());
            if dhi.x <= u0 || dlo.x >= u1 || dhi.y <= v0 || dlo.y >= v1 {
                continue;
            }
            a0 = ((dlo.x - u0) / (u1 - u0)).max(0.0);
            a1 = ((dhi.x - u0) / (u1 - u0)).min(1.0);
            b0 = ((dlo.y - v0) / (v1 - v0)).max(0.0);
            b1 = ((dhi.y - v0) / (v1 - v0)).min(1.0);
            // each end and the restriction's own split parameters ((u1 - u0) / (1 - u0))
            // round within 3 u of the unit interval: 8 EPSILON (16 u) each way covers them
            a0 = (a0 - 8.0 * f64::EPSILON).max(0.0);
            b0 = (b0 - 8.0 * f64::EPSILON).max(0.0);
            a1 = (a1 + 8.0 * f64::EPSILON).min(1.0);
            b1 = (b1 + 8.0 * f64::EPSILON).min(1.0);
        }
        let mut c = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        let mut err = 0.0_f64;
        let p_size = p.x.abs().max(p.y.abs()).max(p.z.abs());
        for prow in patch.rows() {
            for q in prow {
                if q.z == NOZ {
                    return Err(SolidError::Unsupported {
                        operation: OP,
                        reason: "a 2D form",
                    });
                }
                let w = if q.w == NOW { 1.0 } else { q.w };
                // H - p W, each rounded within gamma_2 of its terms
                let t = [q.x - p.x * w, q.y - p.y * w, q.z - p.z * w];
                for (h, o) in [q.x, q.y, q.z].into_iter().zip([p.x, p.y, p.z]) {
                    err = err.max(gamma(2) * (h.abs() + (o * w).abs()));
                }
                for (dst, v) in c.iter_mut().zip([t[0], t[1], t[2], w]) {
                    dst.push(v);
                }
            }
        }
        // the form's error in H and in W, carried through the translation
        err += form_err * (1.0 + p_size) * (1.0 + gamma(2));
        let [cx, cy, cz, cw] = c;
        let h = [
            Bernstein2::new(pu, pv, cx)?,
            Bernstein2::new(pu, pv, cy)?,
            Bernstein2::new(pu, pv, cz)?,
            Bernstein2::new(pu, pv, cw)?,
        ];
        let restricted = (a0, a1, b0, b1) != (0.0, 1.0, 0.0, 1.0);
        let h = if restricted {
            // four de Casteljau levels at most, on the coefficients before them
            let largest = h
                .iter()
                .map(Bernstein2::max_abs_coefficient)
                .fold(0.0_f64, f64::max);
            err += gamma(4 * (2 * (pu + pv) + 4)) * (largest + err);
            h.map(|x| x.restrict(a0, a1, b0, b1))
        } else {
            h
        };
        heap.push(Piece::new(h, err, 0)?);
        made += 1;
    }
    let off = |lower: f64| (lower - slack).max(0.0);
    loop {
        // (the box is inside the patches' range, so some patch overlaps it)
        let nearest = heap.pop().ok_or_else(|| SolidError::sm_err(OP))?;
        if nearest.lower >= want_net || made >= MAX_PATCHES {
            return Ok(off(nearest.lower));
        }
        // the piece's centre is a point of the net over the box: nearer than the target, no
        // bound reaches it
        let [x, y, z, w] = &nearest.h;
        let wc = w.eval(0.5, 0.5);
        let centre = (
            x.eval(0.5, 0.5) / wc,
            y.eval(0.5, 0.5) / wc,
            z.eval(0.5, 0.5) / wc,
        );
        if (centre.0 * centre.0 + centre.1 * centre.1 + centre.2 * centre.2).sqrt() < want_net {
            return Ok(off(nearest.lower));
        }
        for q in nearest.quarters()? {
            heap.push(q);
        }
        made += 4;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
    use crate::query::api_queries::face_bounding_box;
    use crate::topology::brep::p1 as brep_p1;

    fn faces(b: &Brep) -> Vec<FaceId> {
        brep_p1::faces(b, None).unwrap()
    }

    /// The face of the 10 mm box at the origin whose box lies on `x = 10`.
    fn box_side(k: &Kernel, b: &Brep) -> FaceId {
        faces(b)
            .into_iter()
            .find(|&f| {
                let bx = face_bounding_box(k, b, f, true).unwrap();
                (bx.min().x - 10.0).abs() < 1e-9 && (bx.max().x - 10.0).abs() < 1e-9
            })
            .unwrap()
    }

    #[test]
    fn a_plane_face_is_bounded_by_its_distance_and_proves_it() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let f = box_side(&k, &b);
        // straight out of the face: 5; past its edge: to the edge's nearest point, (10, 10, 5),
        // sqrt(25 + 225). The plane's form carries its analytic constants' rounding,
        // gamma_32 (2 M + |O|) (no knot insertion: one span), which the quotient widening
        // scales by the distance: proved to 1e-10 relative here
        for (p, exact) in [
            (Point3::new(15.0, 5.0, 5.0), 5.0),
            (Point3::new(15.0, 25.0, 5.0), 250.0_f64.sqrt()),
        ] {
            let d = face_distance_lower_bound(&k, &b, f, p, exact * (1.0 - 1e-10)).unwrap();
            assert!((exact * (1.0 - 1e-10)..=exact).contains(&d), "{d}");
        }
    }

    #[test]
    fn a_sphere_is_bounded_by_its_distance_from_every_side() {
        let mut k = Kernel::default();
        let b = create_sphere(&mut k, Point3::ZERO, 5.0).unwrap();
        for i in 0..24 {
            let t = f64::from(i) * 0.37;
            let dir = Point3::new(
                t.cos() * (0.3 * t).cos(),
                t.sin() * (0.3 * t).cos(),
                (0.3 * t).sin(),
            );
            let p = dir * 7.0;
            // the sphere's distance from p is 2, over all its faces
            let mut least = f64::INFINITY;
            for f in faces(&b) {
                least =
                    least.min(face_distance_lower_bound(&k, &b, f, p, 2.0 * (1.0 - 1e-6)).unwrap());
            }
            assert!(least <= 2.0 + 1e-12, "{i}: {least} above the distance");
            assert!(least >= 2.0 * (1.0 - 1e-6), "{i}: {least} not proved");
        }
    }

    /// A sphere far from the origin (its centre at 2^20): its form's constants round on the
    /// size of the centre, far above the net's own span, which the analytic term charges
    /// (`gamma_32 (2 M + |O|)`). Asked for more than the distance, the bound subdivides until
    /// its pieces are far finer than that rounding, and must stay at or below the exact
    /// distance (`p - c` is exact, so `|p - c| - r` is within 1e-15 of it). (The translation's
    /// and the subdivisions' own charges, of the same order, keep it below without the term
    /// too: the term is the proof's, see `notes-kprim/HANDOFF.md`. At 2^30 the term leaves the
    /// weights unproved: refused, never over-claimed.)
    #[test]
    fn a_far_sphere_is_bounded_below_its_exact_distance() {
        let mut k = Kernel::default();
        let c = Point3::new(1_048_576.0, -524_288.0, 262_144.0);
        let b = create_sphere(&mut k, c, 1.0).unwrap();
        for i in 0..24 {
            let t = f64::from(i) * 0.37;
            let dir = Point3::new(
                t.cos() * (0.3 * t).cos(),
                t.sin() * (0.3 * t).cos(),
                (0.3 * t).sin(),
            );
            let p = c + dir * 3.0;
            let exact = (p - c).length() - 1.0;
            let mut least = f64::INFINITY;
            for f in faces(&b) {
                least = least.min(face_distance_lower_bound(&k, &b, f, p, exact + 1e-9).unwrap());
            }
            assert!(
                least <= exact - 1e-15,
                "{i}: {least} above the distance {exact}"
            );
        }
    }

    #[test]
    fn a_bound_asked_beyond_the_distance_stays_below_it() {
        let mut k = Kernel::default();
        let b = create_cylinder(&mut k, Point3::ZERO, 3.0, 10.0).unwrap();
        // from the axis at mid height: the side wall is 3 away, the caps 5
        let p = Point3::new(0.0, 0.0, 5.0);
        for f in faces(&b) {
            let d = face_distance_lower_bound(&k, &b, f, p, 100.0).unwrap();
            assert!(d <= 5.0 + 1e-12, "{d}");
        }
        // every point of the wall's circle at mid height is nearest: the pieces a bound
        // within 1e-3 of it takes grow as the inverse square root of that precision
        let least = faces(&b)
            .into_iter()
            .map(|f| face_distance_lower_bound(&k, &b, f, p, 3.0 * (1.0 - 1e-3)).unwrap())
            .fold(f64::INFINITY, f64::min);
        assert!(
            (3.0 * (1.0 - 1e-3)..=3.0 + 1e-12).contains(&least),
            "{least}"
        );
    }

    #[test]
    fn a_point_not_finite_is_refused() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let f = box_side(&k, &b);
        assert!(matches!(
            face_distance_lower_bound(&k, &b, f, Point3::new(f64::NAN, 0.0, 0.0), 1.0),
            Err(SolidError::InvalidInput { .. })
        ));
    }
    use crate::geom::extent2d::Extent2d;
    use crate::nurbs::b_spline_curve::p1::KnotType;
    use crate::nurbs::b_spline_surface::p1::{
        BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
    };

    /// The face's surface bound over half its UV box is the bound of the half: from a point
    /// beyond the half, at least the bound of the whole, and never more than the half's
    /// sampled distance.
    #[test]
    fn a_face_bounded_over_part_of_its_box_takes_the_part() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let f = box_side(&k, &b);
        let surf = b.face(f).unwrap().surface.clone().unwrap();
        let whole = b.face(f).unwrap().uv_domain;
        let (lo, hi) = (whole.min(), whole.max());
        let mut half = Extent2d::default();
        half.set_min_max_coords(lo.x, lo.y, 0.5 * (lo.x + hi.x), hi.y)
            .unwrap();
        // the sampled least distance over the half, from a point off the face's far half
        let probe = |p: Point3| {
            let mut best = f64::INFINITY;
            for i in 0..=200 {
                for j in 0..=200 {
                    let uv = half.evaluate(f64::from(i) / 200.0, f64::from(j) / 200.0);
                    best = best.min(surf.evaluate_point(&k, uv).unwrap().distance(p));
                }
            }
            best
        };
        for p in [Point3::new(15.0, 9.0, 9.0), Point3::new(15.0, 1.0, 1.0)] {
            let sampled = probe(p);
            let all = surface_distance_lower_bound(&k, &surf, Some(&whole), p, 4.0).unwrap();
            let part =
                surface_distance_lower_bound(&k, &surf, Some(&half), p, sampled * (1.0 - 1e-9))
                    .unwrap();
            assert!(
                part >= all - 1e-12 && part <= sampled + 1e-12,
                "{all} {part} {sampled}"
            );
            assert!(part >= sampled * (1.0 - 1e-9) - 1e-9, "{part} {sampled}");
        }
    }

    /// A quadratic sheet of two spans (interior knot at 0.5): the bound reaches the sampled
    /// distance and stays below it.
    #[test]
    fn a_multi_span_spline_is_bounded() {
        let k = Kernel::default();
        let pts: Vec<Point3> = [0.0, 1.0, 3.0, 4.0]
            .iter()
            .zip([0.0, 2.0, -1.0, 0.5])
            .flat_map(|(&x, z)| [Point3::new(x, 0.0, z), Point3::new(x, 2.0, z + 0.3)])
            .collect();
        let s = Surface::from(
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: 2,
                v_degree: 1,
                control_points: &pts,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &[3, 1, 3],
                v_multiplicities: &[2, 2],
                u_knots: &[0.0, 0.5, 1.0],
                v_knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: None,
                uv_domain: None,
            })
            .unwrap(),
        );
        let p = Point3::new(2.0, 1.0, 4.0);
        let dom = s.natural_uv_domain_with_nurb(&k).unwrap();
        let mut sampled = f64::INFINITY;
        for i in 0..=400 {
            for j in 0..=40 {
                let uv = dom.evaluate(f64::from(i) / 400.0, f64::from(j) / 40.0);
                sampled = sampled.min(s.evaluate_point(&k, uv).unwrap().distance(p));
            }
        }
        let d = surface_distance_lower_bound(&k, &s, None, p, sampled * (1.0 - 1e-4)).unwrap();
        assert!(
            (sampled * (1.0 - 1e-4)..=sampled + 1e-12).contains(&d),
            "{d} {sampled}"
        );
    }

    /// A procedural blend evaluates on its definition, which its net is within its bound of:
    /// the net (the unit square in z = 0) is 1 from (0.5, 0.5, 1), and the bound is 1 less the
    /// definition's 1e-3, not more.
    #[test]
    fn a_procedural_blend_is_bounded_less_its_definitions_bound() {
        use crate::blend::fillet_section::section::GnSectionFsg;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_blend::{BlendDefinitionInput, ProceduralBlend};
        let mut k = Kernel::default();
        k.fixes.fillet_gn_section_generator = true;
        k.fixes.fillet_round_as_definition = true;
        let bilinear = |p: [Point3; 4]| {
            BSplineSurface::create_bilinear_surface(p[0], p[1], p[2], p[3]).unwrap()
        };
        let line = |a: Point3, b: Point3| BSplineCurve::create_line_segment(3, a, b).unwrap();
        let (l, r) = (10.0, 2.0);
        let o = Point3::ZERO;
        let a = Surface::from(bilinear([
            o,
            Point3::new(l, 0.0, 0.0),
            Point3::new(0.0, 0.0, l),
            Point3::new(l, 0.0, l),
        ]));
        let b = Surface::from(bilinear([
            o,
            Point3::new(l, 0.0, 0.0),
            Point3::new(0.0, l, 0.0),
            Point3::new(l, l, 0.0),
        ]));
        let net = bilinear([
            o,
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ]);
        let leader = line(Point3::new(0.0, r, r), Point3::new(l, r, r));
        let seed = line(Point3::new(0.0, r / l, 0.0), Point3::new(1.0, r / l, 0.0));
        let input = BlendDefinitionInput {
            supports: [&a, &b],
            sides: [-1.0, 1.0],
            offsets: [r, r],
            law: None,
            order: 2,
            thumbweight: 0.0,
            leader: &leader,
            seeds: [&seed, &seed],
            bound: 1e-3,
            chamfer: None,
        };
        let pb = ProceduralBlend::create(&k, &net, &input, |n, w| {
            GnSectionFsg::is_valid(n as usize, w)
        })
        .unwrap();
        let s = Surface::from(pb);
        let d =
            surface_distance_lower_bound(&k, &s, None, Point3::new(0.5, 0.5, 1.0), 0.99).unwrap();
        assert!((1.0 - 1e-3 - 1e-9..=1.0 - 1e-3 + 1e-12).contains(&d), "{d}");
    }

    #[test]
    fn what_cannot_be_bounded_is_refused() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let f = box_side(&k, &b);
        let surf = b.face(f).unwrap().surface.clone().unwrap();
        let p = Point3::new(20.0, 0.0, 0.0);
        for want in [f64::NAN, -1.0, f64::INFINITY] {
            assert!(matches!(
                surface_distance_lower_bound(&k, &surf, None, p, want),
                Err(SolidError::InvalidInput { .. })
            ));
        }
        // a UV box outside the surface's patches
        let dom = surf.natural_uv_domain_with_nurb(&k).unwrap();
        let (lo, hi) = (dom.min(), dom.max());
        let mut out = Extent2d::default();
        out.set_min_max_coords(hi.x + 1.0, lo.y, hi.x + 2.0, hi.y)
            .unwrap();
        assert!(matches!(
            surface_distance_lower_bound(&k, &surf, Some(&out), p, 1.0),
            Err(SolidError::InvalidInput { .. })
        ));
        // an offset surface has no NURBS form
        let off = Surface::from(
            crate::surface::offset_surface::OffsetSurface::new(&k, 1.0, surf, true, 0).unwrap(),
        );
        assert!(matches!(
            surface_distance_lower_bound(&k, &off, None, p, 1.0),
            Err(SolidError::Unsupported { .. })
        ));
    }

    /// Every point of the wall's circle at mid height is nearest to the axis: a bound asked
    /// to within 1e-12 of it stops at the piece cap, below the distance, and certified.
    #[test]
    fn the_cap_stops_a_bound_below_the_distance() {
        let mut k = Kernel::default();
        let b = create_cylinder(&mut k, Point3::ZERO, 3.0, 10.0).unwrap();
        let p = Point3::new(0.0, 0.0, 5.0);
        let mut least = f64::INFINITY;
        for f in faces(&b) {
            let d = face_distance_lower_bound(&k, &b, f, p, 3.0 * (1.0 - 1e-12)).unwrap();
            assert!(d.is_finite() && d <= 5.0 + 1e-12, "{d}");
            least = least.min(d);
        }
        assert!(
            least < 3.0 * (1.0 - 1e-12),
            "the cap stopped it short: {least}"
        );
    }

    /// A net with a coefficient that is not finite is refused before anything (it was left out
    /// of the boxes, the bound 0 by luck).
    #[test]
    fn a_net_not_finite_is_refused() {
        let k = Kernel::default();
        let nan = Point3::new(f64::NAN, 0.0, 0.0);
        let s = Surface::from(
            BSplineSurface::create_bilinear_surface(
                nan,
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        assert!(!measurable(&k, &s).unwrap());
        assert!(matches!(
            surface_distance_lower_bound(&k, &s, None, Point3::new(0.0, 0.0, 1.0), 0.5),
            Err(SolidError::Unsupported { .. })
        ));
    }

    /// A large net restricted to a tiny box: the restriction's rounding is measured on the
    /// net before it (1e6 across), so the bound stays below the exact distance, 1e-3.
    #[test]
    fn a_large_net_on_a_tiny_box_stays_below_the_distance() {
        let k = Kernel::default();
        let w = 1e6;
        let s = Surface::from(
            BSplineSurface::create_bilinear_surface(
                Point3::new(-w, -w, 0.3),
                Point3::new(w, -w, 0.7),
                Point3::new(-w, w, 0.1),
                Point3::new(w, w, 0.9),
            )
            .unwrap(),
        );
        let dom = s.natural_uv_domain_with_nurb(&k).unwrap();
        let c = dom.evaluate(0.5, 0.5);
        let mut tiny = Extent2d::default();
        tiny.set_min_max_coords(c.x, c.y, c.x + 1e-12, c.y + 1e-12)
            .unwrap();
        let at = s.evaluate_point(&k, c).unwrap();
        let p = at + crate::math::Vec3::new(0.0, 0.0, 1e-3);
        let d = surface_distance_lower_bound(&k, &s, Some(&tiny), p, 0.0).unwrap();
        // the surface over the tiny box is within its size of `at`: no nearer than 1e-3 less it
        assert!(d <= 1e-3 + 1e-12, "{d}");
    }
}
