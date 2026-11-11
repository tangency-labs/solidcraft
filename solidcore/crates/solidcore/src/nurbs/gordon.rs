//! The Gordon surface of a network of curves: u-curves `C_k` (k = 0..=r) crossed by v-curves
//! `D_l` (l = 0..=s), each u-curve crossing every v-curve once.
//!
//! Upstream: none (not upstream; solidcore-native). Sources: W. J. Gordon, "Spline-blended
//! surface interpolation through curve networks", J. Math. Mech. 18 (1969); Piegl and Tiller,
//! *The NURBS Book*, 2nd ed., §10.5 (Gordon surfaces), §9.2.1 (global interpolation) and
//! eq. 9.8 (averaged knots).
//!
//! | Rust       | Upstream |
//! |------------|----------|
//! | [`gordon`] | none     |
//!
//! The surface is `S = L1 + L2 - T` (Gordon's Boolean sum): `L1` skins the u-curves through
//! the v parameters `v_k` of their crossings, `L2` skins the v-curves through the u parameters
//! `u_l`, and `T` is the tensor-product surface through the crossing points `Q_kl` at
//! `(u_l, v_k)`. Each u-curve then lies on `S` at `v = v_k` and each v-curve at `u = u_l`
//! provided the network is compatible: every v-curve crosses all the u-curves at one `u_l`
//! of theirs, and every u-curve all the v-curves at one `v_k`. This module takes the compatible
//! networks only: the crossings are found by the kernel's curve intersector
//! ([`intersect_curves`]), `u_l` and `v_k` are their means (the outer curves' at the curves'
//! ends), and every crossing is proved within the tolerance at those common parameters, else
//! the network is refused naming the pair. The cross degrees are `min(3, count - 1)` with
//! averaged knots; the three surfaces are made compatible (degree elevation, knot insertion)
//! and their nets combined.
//!
//! The result is certified: the distance between each input curve and the surface's iso-curve
//! at its parameter ([`certified_distance`], plus the rounding of extracting the iso-curve by
//! knot insertion: `γ_(12 (d + 1))` of twice the net's largest coordinate per coordinate, `d`
//! the cross degree, a coordinate error `e` moving a point by at most `2 e`), all counted in the
//! network's own frame (minus its first control point, each moved coordinate rounding once) so
//! the bound follows the network's size, not its distance from the origin; the largest such
//! bound is returned with the surface. Past the tolerance the network is refused.
//!
//! Deviations: none (no upstream counterpart). Non-rational 3D curves only. No recursion.

use core::cmp::Ordering;

use crate::curve::curve::Curve as KCurve;
use crate::error::intersect::IntersectFailure;
use crate::error::{Result, SolidError};
use crate::intersect::api_intersectors::intersect_curves;
use crate::kernel::Kernel;
use crate::limits::try_reserve_total;
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::basis_basic::COMPATIBLE_KNOT_TOL;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::crv_deviation::{certified_distance, interpolation_bound};
use crate::nurbs::crv_fit::p1::{interp_given_params, interp_knot_vector};
use crate::nurbs::crv_tool::p2::make_compatible_knot_tol;
use crate::nurbs::knots::{KnotVector, Side};
use crate::nurbs::math::{gamma, CPoint};
use crate::nurbs::srf_basic::{Direction, Surface};
use crate::nurbs::srf_tool::p1::extract_iso_crv;
use crate::nurbs::srf_tool::p4::make_compatible_uv;

const OP: &str = "nurbs::gordon::gordon";

/// The fewest curves of a network each way.
pub const MIN_CURVES: usize = 2;
/// The most curves of a network each way.
pub const MAX_CURVES: usize = 64;

/// The highest cross degree (the degree across the curves of one family).
const CROSS_DEGREE: usize = 3;

/// A Gordon surface and the bound it is certified to: no input curve is farther than `bound`
/// from the surface's iso-curve at its parameter.
#[derive(Debug, Clone)]
pub struct Gordon {
    /// The surface: u along the u-curves, v along the v-curves.
    pub surface: BSplineSurface,
    /// The largest certified distance between an input curve and its iso-curve.
    pub bound: f64,
}

/// Why a network has no Gordon surface here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NetworkRefusal {
    /// A curve is rational: the Boolean sum of control nets is exact for polynomial curves
    /// only.
    Rational,
    /// u-curve `u` and v-curve `v` do not cross once: `count` crossings found.
    Crossings {
        /// The u-curve.
        u: usize,
        /// The v-curve.
        v: usize,
        /// How many crossings the intersector found.
        count: usize,
    },
    /// The crossings along one family do not increase from curve to curve (the curves are
    /// not in order, or do not run one way).
    Order,
    /// At the network's common parameters u-curve `u` and v-curve `v` are not proved within
    /// the tolerance: `gap` bounds how far apart they are there.
    Gap {
        /// The u-curve.
        u: usize,
        /// The v-curve.
        v: usize,
        /// The certified bound of their distance at the common parameters.
        gap: f64,
    },
    /// The surface is built but an input curve is not proved within the tolerance of it:
    /// `bound`, the largest certified distance.
    PastTolerance {
        /// The largest certified distance.
        bound: f64,
    },
}

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// `c`'s NURB, in polynomial form when its weights are all one value: a curve whose weights are
/// equal is the polynomial curve on its points divided by that weight, exactly (a record
/// spline's kernel curve carries a weight of 1 on every point). Only unequal weights make it
/// rational.
fn nurb_of(c: &BSplineCurve) -> Result<Curve> {
    let mut nurb = c
        .nurb
        .clone()
        .ok_or_else(|| bad("a curve without its NURB"))?;
    if nurb.is_rational() {
        let first = nurb.cpts().first().map(|p| p.w);
        if nurb.cpts().iter().all(|p| Some(p.w) == first) {
            nurb.make_non_rat();
        }
    }
    Ok(nurb)
}

/// `c` as the kernel's curve, for the intersector.
fn kernel_curve(c: &Curve) -> KCurve {
    KCurve::from(BSplineCurve::from_gw_nurb(3, c))
}

/// The one crossing of `a` and `b` (its parameter on each), or how many there are.
fn crossing(k: &Kernel, a: &Curve, b: &Curve) -> Result<std::result::Result<(f64, f64), usize>> {
    match intersect_curves(k, &kernel_curve(a), &kernel_curve(b), false) {
        Ok(found) => match (found.params1.as_slice(), found.params2.as_slice()) {
            ([ta], [tb]) => Ok(Ok((*ta, *tb))),
            (all, _) => Ok(Err(all.len())),
        },
        Err(e)
            if matches!(
                e.intersect_failure(),
                Some(IntersectFailure::Failed("the curves do not intersect"))
            ) =>
        {
            Ok(Err(0))
        }
        Err(e) => Err(e),
    }
}

/// `(x, y, z)` of a polynomial control point.
fn point(c: CPoint) -> Vec3 {
    Vec3::new(c.x, c.y, c.z)
}

/// The curve of degree `p` through `points` at `params` on `knots` (global interpolation).
fn through(points: &[Vec3], params: &[f64], knots: &[f64], p: usize) -> Result<Curve> {
    interp_given_params(points, params, &KnotVector::from_slice(knots)?, p)
}

/// Whether `t` increases strictly.
fn increasing(t: &[f64]) -> bool {
    t.windows(2).all(|w| matches!(w, [a, b] if a < b))
}

/// The surface whose net is `net[i][j]` (`i` along u), degrees `p`, `q`, knots `u`, `v`.
fn surface(net: Vec<Vec<Vec3>>, p: usize, q: usize, u: &[f64], v: &[f64]) -> Result<Surface> {
    let net = net
        .into_iter()
        .map(|row| row.into_iter().map(CPoint::from_point).collect())
        .collect();
    Surface::new(net, p, q, u.to_vec(), v.to_vec())
}

/// `c` moved by minus `o`, into the network's own frame.
fn moved(c: &Curve, o: CPoint) -> Result<Curve> {
    let pw = c
        .cpts()
        .iter()
        .map(|q| CPoint::new(q.x - o.x, q.y - o.y, q.z - o.z, q.w))
        .collect();
    Curve::new(pw, c.degree(), c.knots().to_vec())
}

/// `s` moved by minus `o`, into the network's own frame.
fn moved_surface(s: &Surface, o: CPoint) -> Result<Surface> {
    let net = s
        .rows()
        .map(|row| {
            row.iter()
                .map(|q| CPoint::new(q.x - o.x, q.y - o.y, q.z - o.z, q.w))
                .collect()
        })
        .collect();
    let (p, q) = s.degrees();
    Surface::new(net, p, q, s.u_knots().to_vec(), s.v_knots().to_vec())
}

/// The largest coordinate of `points`.
fn largest<'a>(points: impl Iterator<Item = &'a CPoint>) -> f64 {
    points
        .map(|c| c.x.abs().max(c.y.abs()).max(c.z.abs()))
        .fold(0.0_f64, f64::max)
}

/// An empty `Vec` with room for `count + 1` values, reserved fallibly: a count past the
/// addressable or the allocation bound ([`try_reserve_total`]) is refused.
fn room<T>(count: usize) -> Result<Vec<T>> {
    let len = count
        .checked_add(1)
        .ok_or_else(|| bad("a count past the addressable"))?;
    let mut v = Vec::new();
    if !try_reserve_total(&mut v, len) {
        return Err(bad("an allocation past the bound"));
    }
    Ok(v)
}

/// Whether `x` is at most `tol`; a NaN is not.
fn within(x: f64, tol: f64) -> bool {
    matches!(x.partial_cmp(&tol), Some(Ordering::Less | Ordering::Equal))
}

/// The Gordon surface of the u-curves `us` and the v-curves `vs` (module docs), its crossings
/// and its curves proved within `tol` of it.
///
/// # Errors
///
/// `InvalidInput` for fewer than [`MIN_CURVES`] or more than [`MAX_CURVES`] curves a family, a
/// curve without its NURB or `tol` not finite and positive; those of the intersector, the
/// interpolation, the compatibility and the certificates.
pub fn gordon(
    k: &Kernel,
    us: &[BSplineCurve],
    vs: &[BSplineCurve],
    tol: f64,
) -> Result<std::result::Result<Gordon, NetworkRefusal>> {
    let family = MIN_CURVES..=MAX_CURVES;
    if !family.contains(&us.len()) || !family.contains(&vs.len()) {
        return Err(bad("a family of fewer than two or more than 64 curves"));
    }
    if !(tol.is_finite() && tol > 0.0) {
        return Err(bad("a tolerance not finite and positive"));
    }
    let mut cu: Vec<Curve> = us.iter().map(nurb_of).collect::<Result<_>>()?;
    let mut dv: Vec<Curve> = vs.iter().map(nurb_of).collect::<Result<_>>()?;
    if cu.iter().chain(&dv).any(Curve::is_rational) {
        return Ok(Err(NetworkRefusal::Rational));
    }
    // each family on one degree, knot vector and domain, exactly
    make_compatible_knot_tol(&mut cu, COMPATIBLE_KNOT_TOL)?;
    make_compatible_knot_tol(&mut dv, COMPATIBLE_KNOT_TOL)?;
    let (r, s) = (cu.len() - 1, dv.len() - 1);

    // the crossings: on u-curve kk at u_cross[kk][l], on v-curve l at v_cross[kk][l]
    let mut u_cross = vec![vec![0.0; s + 1]; r + 1];
    let mut v_cross = vec![vec![0.0; s + 1]; r + 1];
    for (kk, c) in cu.iter().enumerate() {
        for (l, d) in dv.iter().enumerate() {
            match crossing(k, c, d)? {
                Ok((tu, tv)) => {
                    if let (Some(a), Some(b)) = (
                        u_cross.get_mut(kk).and_then(|row| row.get_mut(l)),
                        v_cross.get_mut(kk).and_then(|row| row.get_mut(l)),
                    ) {
                        *a = tu;
                        *b = tv;
                    }
                }
                Err(count) => {
                    return Ok(Err(NetworkRefusal::Crossings { u: kk, v: l, count }));
                }
            }
        }
    }
    // the common parameters: the means, the outer curves' at the families' ends
    let mut u_at: Vec<f64> = (0..=s)
        .map(|l| u_cross.iter().filter_map(|row| row.get(l)).sum::<f64>() / (r + 1) as f64)
        .collect();
    let mut v_at: Vec<f64> = (0..=r)
        .map(|kk| v_cross.get(kk).map_or(0.0, |row| row.iter().sum::<f64>()) / (s + 1) as f64)
        .collect();
    let (Some(c0), Some(d0)) = (cu.first(), dv.first()) else {
        return Err(bad("an empty family"));
    };
    // in order: each family's crossings increase from curve to curve
    if !increasing(&u_at) || !increasing(&v_at) {
        return Ok(Err(NetworkRefusal::Order));
    }
    let ((ua, ub), (va, vb)) = (c0.param_bounds(), d0.param_bounds());
    if let Some(first) = u_at.first_mut() {
        *first = ua;
    }
    if let Some(last) = u_at.last_mut() {
        *last = ub;
    }
    if let Some(first) = v_at.first_mut() {
        *first = va;
    }
    if let Some(last) = v_at.last_mut() {
        *last = vb;
    }
    if !increasing(&u_at) || !increasing(&v_at) {
        return Ok(Err(NetworkRefusal::Order));
    }

    // The certificates are counted in the network's own frame (minus the first u-curve's
    // first control point), so their rounding follows the network's size, not its distance
    // from the origin: each moved coordinate rounds once (`γ_1` of the moved value), moving a
    // point by at most `2 γ_1 M`, `M` the largest moved coordinate.
    let origin = c0
        .cpts()
        .first()
        .copied()
        .ok_or_else(|| bad("a curve without points"))?;
    let cu_local: Vec<Curve> = cu.iter().map(|c| moved(c, origin)).collect::<Result<_>>()?;
    let dv_local: Vec<Curve> = dv.iter().map(|d| moved(d, origin)).collect::<Result<_>>()?;
    let size = largest(cu_local.iter().chain(&dv_local).flat_map(Curve::cpts));
    let move_err = 2.0 * gamma(1) * size;

    // the crossing points at the common parameters, each proved within tol of both curves:
    // |C(u) - D(v)| <= |C(u) - q| + |D(v) - q| for any q, both bounded in the network's frame
    let mut grid: Vec<Vec<Vec3>> = room(r)?;
    for (kk, ((c, c_local), &v)) in cu.iter().zip(&cu_local).zip(&v_at).enumerate() {
        let mut row = room(s)?;
        for (l, (d_local, &u)) in dv_local.iter().zip(&u_at).enumerate() {
            let q = c.eval(u, Side::Left)?;
            let q_local = c_local.eval(u, Side::Left)?;
            let on_c = interpolation_bound(c_local, &[u], &[q_local])?;
            let on_d = interpolation_bound(d_local, &[v], &[q_local])?;
            let gap = (on_c + on_d + 2.0 * move_err) * (1.0 + gamma(2));
            if !within(gap, tol) {
                return Ok(Err(NetworkRefusal::Gap { u: kk, v: l, gap }));
            }
            row.push(q);
        }
        grid.push(row);
    }

    let (pu, qv) = (s.min(CROSS_DEGREE), r.min(CROSS_DEGREE));
    let u_knots = interp_knot_vector(&u_at, pu)?;
    let v_knots = interp_knot_vector(&v_at, qv)?;

    // L1: the u-curves skinned in v, column by column of their (common) control points
    let (p, cu_knots) = (c0.degree(), c0.knots().to_vec());
    let n = c0.n();
    let mut l1 = room(n)?;
    for i in 0..=n {
        let column: Vec<Vec3> = cu
            .iter()
            .map(|c| c.cpts().get(i).copied().map(point))
            .collect::<Option<_>>()
            .ok_or_else(|| bad("curves of one family with different nets"))?;
        let skin = through(&column, &v_at, &v_knots, qv)?;
        l1.push(skin.cpts().iter().copied().map(point).collect());
    }
    let l1 = surface(l1, p, qv, &cu_knots, &v_knots)?;

    // L2: the v-curves skinned in u, row by row of their control points
    let (q, dv_knots) = (d0.degree(), d0.knots().to_vec());
    let m = d0.n();
    let mut by_j: Vec<Vec<Vec3>> = room(m)?;
    for j in 0..=m {
        let column: Vec<Vec3> = dv
            .iter()
            .map(|d| d.cpts().get(j).copied().map(point))
            .collect::<Option<_>>()
            .ok_or_else(|| bad("curves of one family with different nets"))?;
        let skin = through(&column, &u_at, &u_knots, pu)?;
        by_j.push(skin.cpts().iter().copied().map(point).collect());
    }
    let l2_net: Vec<Vec<Vec3>> = (0..=s)
        .map(|i| by_j.iter().filter_map(|col| col.get(i).copied()).collect())
        .collect();
    let l2 = surface(l2_net, pu, q, &u_knots, &dv_knots)?;

    // T: the crossing points interpolated, along u then along v
    let mut along_u: Vec<Vec<Vec3>> = room(r)?;
    for row in &grid {
        let c = through(row, &u_at, &u_knots, pu)?;
        along_u.push(c.cpts().iter().copied().map(point).collect());
    }
    let mut t_net: Vec<Vec<Vec3>> = room(s)?;
    for i in 0..=s {
        let column: Vec<Vec3> = along_u
            .iter()
            .filter_map(|row| row.get(i).copied())
            .collect();
        let c = through(&column, &v_at, &v_knots, qv)?;
        t_net.push(c.cpts().iter().copied().map(point).collect());
    }
    let t = surface(t_net, pu, qv, &u_knots, &v_knots)?;

    // S = L1 + L2 - T on one degree and knot vector each way
    let mut three = [l1, l2, t];
    make_compatible_uv(&mut three)?;
    let [l1, l2, t] = &three;
    let (nn, mm) = (l1.n(), l1.m());
    let mut net = room(nn)?;
    for i in 0..=nn {
        let mut row = room(mm)?;
        for j in 0..=mm {
            let (a, b, c) = (
                point(l1.cpt(i, j)?),
                point(l2.cpt(i, j)?),
                point(t.cpt(i, j)?),
            );
            row.push(Vec3::new(a.x + b.x - c.x, a.y + b.y - c.y, a.z + b.z - c.z));
        }
        net.push(row);
    }
    let (sp, sq) = l1.degrees();
    let summed = surface(net, sp, sq, l1.u_knots(), l1.v_knots())?;
    let made = BSplineSurface::from_gw_nurb(&summed);
    let s_nurb = made
        .nurb
        .as_ref()
        .ok_or_else(|| bad("a surface without its NURB"))?;

    // the certificate: each input curve against its iso-curve, in the network's frame; the
    // extraction's rounding and both moves included
    let s_local = moved_surface(s_nurb, origin)?;
    let s_size = largest(s_local.rows().flatten()).max(size);
    let s_move = 2.0 * gamma(1) * s_size;
    let (dp, dq) = s_local.degrees();
    let mut bound = 0.0_f64;
    for (c, &v) in cu_local.iter().zip(&v_at) {
        let iso = extract_iso_crv(&s_local, v, Direction::U)?;
        let extraction = 4.0 * gamma(12 * (dq + 1)) * s_size;
        let d = (certified_distance(&iso, c)? + extraction + s_move + move_err) * (1.0 + gamma(3));
        bound = bound.max(d);
    }
    for (d, &u) in dv_local.iter().zip(&u_at) {
        let iso = extract_iso_crv(&s_local, u, Direction::V)?;
        let extraction = 4.0 * gamma(12 * (dp + 1)) * s_size;
        let e = (certified_distance(&iso, d)? + extraction + s_move + move_err) * (1.0 + gamma(3));
        bound = bound.max(e);
    }
    if !within(bound, tol) {
        return Ok(Err(NetworkRefusal::PastTolerance { bound }));
    }
    Ok(Ok(Gordon {
        surface: made,
        bound,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The polynomial curve of degree `p` on `points` with the clamped uniform knots.
    fn spline(points: &[[f64; 3]], p: usize) -> BSplineCurve {
        let n = points.len();
        let mut knots = vec![0.0; p + 1];
        for i in 1..n - p {
            knots.push(i as f64 / (n - p) as f64);
        }
        knots.extend(vec![1.0; p + 1]);
        let pw = points
            .iter()
            .map(|q| CPoint::from_point(Vec3::new(q[0], q[1], q[2])))
            .collect();
        BSplineCurve::from_gw_nurb(3, &Curve::new(pw, p, knots).unwrap())
    }

    fn line(a: [f64; 3], b: [f64; 3]) -> BSplineCurve {
        spline(&[a, b], 1)
    }

    fn at(s: &Surface, u: f64, v: f64) -> Vec3 {
        let iso = extract_iso_crv(s, v, Direction::U).unwrap();
        iso.eval(u, Side::Left).unwrap()
    }

    #[test]
    fn a_network_of_lines_is_the_bilinear_patch() {
        let k = Kernel::default();
        let p = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [10.0, 10.0, 5.0],
        ];
        let us = [line(p[0], p[1]), line(p[2], p[3])];
        let vs = [line(p[0], p[2]), line(p[1], p[3])];
        let g = gordon(&k, &us, &vs, 1e-6).unwrap().unwrap();
        assert!(g.bound <= 1e-9, "{}", g.bound);
        let s = g.surface.nurb.as_ref().unwrap();
        for (a, b) in [(0.0, 0.0), (0.3, 0.7), (1.0, 0.5), (0.5, 1.0)] {
            let bilinear = |i: usize| {
                (1.0 - b) * ((1.0 - a) * p[0][i] + a * p[1][i])
                    + b * ((1.0 - a) * p[2][i] + a * p[3][i])
            };
            let q = at(s, a, b);
            assert!(
                (q.x - bilinear(0)).abs() < 1e-9
                    && (q.y - bilinear(1)).abs() < 1e-9
                    && (q.z - bilinear(2)).abs() < 1e-9,
                "{q:?} at ({a}, {b})"
            );
        }
    }

    #[test]
    fn a_curved_network_holds_its_curves() {
        let k = Kernel::default();
        // three u-curves and three v-curves of the saddle z = (x - 5)(y - 5) / 10, crossing at
        // their middles (its sections along x and y are lines: each curve is exact)
        let z = |x: f64, y: f64| (x - 5.0) * (y - 5.0) / 10.0;
        let u_curve = |y: f64| {
            spline(
                &[
                    [0.0, y, z(0.0, y)],
                    [5.0, y, z(5.0, y)],
                    [10.0, y, z(10.0, y)],
                ],
                1,
            )
        };
        let v_curve = |x: f64| {
            spline(
                &[
                    [x, 0.0, z(x, 0.0)],
                    [x, 5.0, z(x, 5.0)],
                    [x, 10.0, z(x, 10.0)],
                ],
                1,
            )
        };
        let us = [u_curve(0.0), u_curve(5.0), u_curve(10.0)];
        let vs = [v_curve(0.0), v_curve(5.0), v_curve(10.0)];
        let g = gordon(&k, &us, &vs, 1e-6).unwrap().unwrap();
        assert!(g.bound <= 1e-6, "{}", g.bound);
        // every input curve on the surface at its parameter, certified
        let s = g.surface.nurb.as_ref().unwrap();
        for (c, v) in us.iter().zip([0.0, 0.5, 1.0]) {
            let iso = extract_iso_crv(s, v, Direction::U).unwrap();
            assert!(certified_distance(&iso, c.nurb.as_ref().unwrap()).unwrap() <= 1e-9);
        }
    }

    #[test]
    fn mismatched_networks_are_refused() {
        let k = Kernel::default();
        let p = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [10.0, 10.0, 0.0],
        ];
        // the second v-curve stops short of the second u-curve: no crossing
        let us = [line(p[0], p[1]), line(p[2], p[3])];
        let vs = [line(p[0], p[2]), line(p[1], [10.0, 5.0, 0.0])];
        assert_eq!(
            gordon(&k, &us, &vs, 1e-6).unwrap().unwrap_err(),
            NetworkRefusal::Crossings {
                u: 1,
                v: 1,
                count: 0
            }
        );
        // the u-curves listed out of order
        let vs = [line(p[0], p[2]), line(p[1], p[3])];
        let reversed = [line(p[2], p[3]), line(p[0], p[1])];
        assert_eq!(
            gordon(&k, &reversed, &vs, 1e-6).unwrap().unwrap_err(),
            NetworkRefusal::Order
        );
        // a v-curve crossing the u-curves at different parameters: a gap at the common ones
        let us = [line(p[0], p[1]), line(p[2], p[3])];
        let slanted = [line(p[0], p[2]), line([10.0, 0.0, 0.0], [6.0, 10.0, 0.0])];
        assert!(matches!(
            gordon(&k, &us, &slanted, 1e-6).unwrap().unwrap_err(),
            NetworkRefusal::Gap { .. } | NetworkRefusal::Crossings { .. }
        ));
        // too few curves
        assert!(gordon(&k, &us[..1], &vs, 1e-6).is_err());
    }

    #[test]
    fn a_patchs_iso_curves_give_the_patch_back() {
        // a polynomial patch, cubic in u with two interior knots and a cubic Bezier in v: four
        // u-curves skinned by cubics reproduce its v, and L2 = T exactly, so S is the patch
        let k = Kernel::default();
        let net: Vec<Vec<CPoint>> = (0..6)
            .map(|i| {
                (0..4)
                    .map(|j| {
                        let z = f64::from((i * 7 + j * 3) % 5) - 2.0;
                        CPoint::from_point(Vec3::new(2.0 * f64::from(i), 3.0 * f64::from(j), z))
                    })
                    .collect()
            })
            .collect();
        let third = 1.0 / 3.0;
        let patch = Surface::new(
            net,
            3,
            3,
            vec![0.0, 0.0, 0.0, 0.0, third, 2.0 * third, 1.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let iso = |t: f64, dir: Direction| {
            BSplineCurve::from_gw_nurb(3, &extract_iso_crv(&patch, t, dir).unwrap())
        };
        let us: Vec<BSplineCurve> = [0.0, 0.3, 0.7, 1.0].map(|v| iso(v, Direction::U)).to_vec();
        let vs: Vec<BSplineCurve> = [0.0, 0.25, 0.6, 1.0].map(|u| iso(u, Direction::V)).to_vec();
        let g = gordon(&k, &us, &vs, 1e-6).unwrap().unwrap();
        assert!(g.bound <= 1e-9, "{}", g.bound);
        let s = g.surface.nurb.as_ref().unwrap();
        for i in 0..=10 {
            for j in 0..=10 {
                let (a, b) = (f64::from(i) / 10.0, f64::from(j) / 10.0);
                let (p, q) = (at(s, a, b), at(&patch, a, b));
                assert!((p - q).length() < 1e-9, "{p:?} against {q:?} at ({a}, {b})");
            }
        }
    }

    #[test]
    fn a_closed_grid_meets_at_its_corners_once() {
        // three lines each way on the rectangle [0, 10] x [0, 20]: the outer curves meet at
        // their ends, and the inner ones end on the outer ones
        let k = Kernel::default();
        let us: Vec<BSplineCurve> = [0.0, 5.0, 20.0]
            .map(|y| line([0.0, y, 0.0], [10.0, y, 0.0]))
            .to_vec();
        let vs: Vec<BSplineCurve> = [0.0, 4.0, 10.0]
            .map(|x| line([x, 0.0, 0.0], [x, 20.0, 0.0]))
            .to_vec();
        let g = gordon(&k, &us, &vs, 1e-6).unwrap().unwrap();
        assert!(g.bound <= 1e-9, "{}", g.bound);
        let s = g.surface.nurb.as_ref().unwrap();
        for (a, b) in [(0.0, 0.0), (1.0, 1.0), (0.4, 0.25), (0.7, 0.9)] {
            let p = at(s, a, b);
            assert!(p.z.abs() < 1e-9 && (0.0..=10.0).contains(&p.x) && (0.0..=20.0).contains(&p.y));
        }
    }

    #[test]
    fn the_bound_follows_the_networks_size_far_from_the_origin() {
        // the bilinear network far out: its bound follows its own size, not its coordinates
        let k = Kernel::default();
        // 2e5 out: counted on the coordinates, the extraction's rounding alone would be
        // 4 γ_24 2e5 ≈ 2e-9; on the network's size (10) it is below 1e-13, and the nets' own
        // rounding there about 1e-10
        let o = [1.0e5, -2.0e5, 3.0e4];
        let p = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [10.0, 10.0, 5.0],
        ]
        .map(|q| [q[0] + o[0], q[1] + o[1], q[2] + o[2]]);
        let us = [line(p[0], p[1]), line(p[2], p[3])];
        let vs = [line(p[0], p[2]), line(p[1], p[3])];
        let g = gordon(&k, &us, &vs, 1e-6).unwrap().unwrap();
        assert!(g.bound <= 5e-10, "{}", g.bound);
    }

    #[test]
    fn curves_weighted_one_throughout_are_polynomial() {
        // a record spline's kernel curve carries w = 1 on every point: not rational
        let k = Kernel::default();
        let weighted = |a: [f64; 3], b: [f64; 3]| {
            let pw = [a, b]
                .iter()
                .map(|q| CPoint::new(q[0], q[1], q[2], 1.0))
                .collect();
            BSplineCurve::from_gw_nurb(3, &Curve::new(pw, 1, vec![0.0, 0.0, 1.0, 1.0]).unwrap())
        };
        let p = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [10.0, 10.0, 5.0],
        ];
        let us = [weighted(p[0], p[1]), weighted(p[2], p[3])];
        let vs = [line(p[0], p[2]), line(p[1], p[3])];
        let g = gordon(&k, &us, &vs, 1e-6).unwrap().unwrap();
        assert!(g.bound <= 1e-9, "{}", g.bound);
        // unequal weights are rational, and refused
        let pw = vec![
            CPoint::new(0.0, 0.0, 0.0, 1.0),
            CPoint::new(10.0, 0.0, 0.0, 2.0),
        ];
        let rational =
            BSplineCurve::from_gw_nurb(3, &Curve::new(pw, 1, vec![0.0, 0.0, 1.0, 1.0]).unwrap());
        assert_eq!(
            gordon(&k, &[rational, us[1].clone()], &vs, 1e-6)
                .unwrap()
                .unwrap_err(),
            NetworkRefusal::Rational
        );
    }
}
