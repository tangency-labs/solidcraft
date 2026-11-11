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
//! the cross degree, a coordinate error `e` moving a point by at most `2 e`), and the largest
//! such bound is returned with the surface. Past the tolerance the network is refused.
//!
//! Deviations: none (no upstream counterpart). Non-rational 3D curves only. No recursion.

use crate::curve::curve::Curve as KCurve;
use crate::error::intersect::IntersectFailure;
use crate::error::{Result, SolidError};
use crate::intersect::api_intersectors::intersect_curves;
use crate::kernel::Kernel;
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

/// `c`'s NURB.
fn nurb_of(c: &BSplineCurve) -> Result<Curve> {
    c.nurb
        .clone()
        .ok_or_else(|| bad("a curve without its NURB"))
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

    // the crossing points at the common parameters, each proved within tol of both curves
    let mut grid: Vec<Vec<Vec3>> = Vec::with_capacity(r + 1);
    for (kk, (c, &v)) in cu.iter().zip(&v_at).enumerate() {
        let mut row = Vec::with_capacity(s + 1);
        for (l, (d, &u)) in dv.iter().zip(&u_at).enumerate() {
            let q = c.eval(u, Side::Left)?;
            let on_c = interpolation_bound(c, &[u], &[q])?;
            let on_d = interpolation_bound(d, &[v], &[q])?;
            let gap = (on_c + on_d) * (1.0 + gamma(1));
            if !(gap <= tol) {
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
    let mut l1 = Vec::with_capacity(n + 1);
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
    let mut by_j: Vec<Vec<Vec3>> = Vec::with_capacity(m + 1);
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
    let mut along_u: Vec<Vec<Vec3>> = Vec::with_capacity(r + 1);
    for row in &grid {
        let c = through(row, &u_at, &u_knots, pu)?;
        along_u.push(c.cpts().iter().copied().map(point).collect());
    }
    let mut t_net: Vec<Vec<Vec3>> = Vec::with_capacity(s + 1);
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
    let mut net = Vec::with_capacity(nn + 1);
    for i in 0..=nn {
        let mut row = Vec::with_capacity(mm + 1);
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

    // the certificate: each input curve against its iso-curve, extraction rounding included
    let largest = s_nurb
        .cpts()
        .iter()
        .flatten()
        .map(|c| c.x.abs().max(c.y.abs()).max(c.z.abs()))
        .fold(0.0_f64, f64::max);
    let (dp, dq) = s_nurb.degrees();
    let mut bound = 0.0_f64;
    for (c, &v) in cu.iter().zip(&v_at) {
        let iso = extract_iso_crv(s_nurb, v, Direction::U)?;
        let extraction = 4.0 * gamma(12 * (dq + 1)) * largest;
        let d = (certified_distance(&iso, c)? + extraction) * (1.0 + gamma(1));
        bound = bound.max(d);
    }
    for (d, &u) in dv.iter().zip(&u_at) {
        let iso = extract_iso_crv(s_nurb, u, Direction::V)?;
        let extraction = 4.0 * gamma(12 * (dp + 1)) * largest;
        let e = (certified_distance(&iso, d)? + extraction) * (1.0 + gamma(1));
        bound = bound.max(e);
    }
    if !(bound <= tol) {
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
}
