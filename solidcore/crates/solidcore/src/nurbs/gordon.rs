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
//! of theirs, and every u-curve all the v-curves at one `v_k`. The crossings are found by the
//! kernel's curve intersector ([`intersect_curves`]), `u_l` and `v_k` are their means (the
//! outer curves' at the curves' ends), and every crossing is proved within the tolerance at
//! those common parameters. A network whose crossings are not there (two curves through the
//! same stations at different heights, each parameterised by its own chord lengths) is
//! refitted first: each curve of a family that is off is sampled along its monotone map from
//! the common parameters to its own crossing parameters (Fritsch and Butland's monotone cubic)
//! and interpolated at the common ones, so its crossings land there exactly; the surface is
//! built from the refits, and each input curve is proved within the bound of the surface span
//! by span between its crossings ([`crate::nurbs::certificate::span_bound`], geometric: no
//! parametric correspondence trusted), the refit's points doubled where a span is past the
//! tolerance. A compatible network is built exactly as before. Refused, naming why: a pair not
//! crossing once, crossings out of order along a family or a curve, an outer curve crossing
//! short of a curve's end (the gap at the common parameters), past the tolerance. The cross degrees are `min(3, count - 1)` with
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
use crate::geom::vector2d::Point2;
use crate::intersect::api_intersectors::intersect_curves;
use crate::kernel::Kernel;
use crate::limits::try_reserve_total;
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::basis_basic::COMPATIBLE_KNOT_TOL;
use crate::nurbs::certificate::{span_bound, Node, RailSpans};
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
pub(crate) fn nurb_of(c: &BSplineCurve) -> Result<Curve> {
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
pub(crate) fn moved(c: &Curve, o: CPoint) -> Result<Curve> {
    let pw = c
        .cpts()
        .iter()
        .map(|q| CPoint::new(q.x - o.x, q.y - o.y, q.z - o.z, q.w))
        .collect();
    Curve::new(pw, c.degree(), c.knots().to_vec())
}

/// `s` moved by minus `o`, into the network's own frame.
pub(crate) fn moved_surface(s: &Surface, o: CPoint) -> Result<Surface> {
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
pub(crate) fn largest<'a>(points: impl Iterator<Item = &'a CPoint>) -> f64 {
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
pub(crate) fn within(x: f64, tol: f64) -> bool {
    matches!(x.partial_cmp(&tol), Some(Ordering::Less | Ordering::Equal))
}

/// A network prepared for its Gordon surface: each family on one degree, knot vector and
/// domain; the crossings (on u-curve `kk` at `u_cross[kk][l]`, on v-curve `l` at
/// `v_cross[kk][l]`); and the common parameters (the means, the outer curves' at the families'
/// ends), in order.
struct Network {
    cu: Vec<Curve>,
    dv: Vec<Curve>,
    u_cross: Vec<Vec<f64>>,
    v_cross: Vec<Vec<f64>>,
    u_at: Vec<f64>,
    v_at: Vec<f64>,
}

/// The network of `us` and `vs` ([`Network`]), or why it has no Gordon surface: a rational
/// curve, a pair not crossing once, crossings out of order.
fn network(
    k: &Kernel,
    us: &[BSplineCurve],
    vs: &[BSplineCurve],
) -> Result<std::result::Result<Network, NetworkRefusal>> {
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

    Ok(Ok(Network {
        cu,
        dv,
        u_cross,
        v_cross,
        u_at,
        v_at,
    }))
}

/// The Gordon surface of the u-curves `us` and the v-curves `vs` (module docs), its crossings
/// and its curves proved within `tol` of it. A network crossing at common parameters is built
/// as it is ([`build`]); one whose crossings are not at common parameters (a gap there, though
/// each pair crosses once and in order) is built from its families refitted so that they are
/// ([`refitted`]), each input curve then proved within the bound of the surface.
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
    match build(k, us, vs, tol)? {
        Err(gap @ NetworkRefusal::Gap { .. }) => refitted(k, us, vs, tol, gap),
        other => Ok(other),
    }
}

/// The Gordon surface of a network crossing at its common parameters (module docs): refused
/// with [`NetworkRefusal::Gap`] where a crossing is not within `tol` there.
///
/// # Errors
///
/// As [`gordon`].
fn build(
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
    let Network {
        cu,
        dv,
        u_cross: _,
        v_cross: _,
        u_at,
        v_at,
    } = match network(k, us, vs)? {
        Ok(n) => n,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let (r, s) = (cu.len() - 1, dv.len() - 1);
    let (Some(c0), Some(d0)) = (cu.first(), dv.first()) else {
        return Err(bad("an empty family"));
    };
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

/// The most points a span between two crossings is refitted through, doubling from
/// [`REFIT_FIRST`]: a resource bound.
const REFIT_MOST: usize = 64;
/// The points a span between two crossings is first refitted through.
const REFIT_FIRST: usize = 8;

/// The monotone cubic through `(xs[i], ys[i])` (both increasing) at `t`: Hermite pieces with
/// the Fritsch-Butland slopes (F. N. Fritsch and J. Butland, "A method for constructing local
/// monotone piecewise cubic interpolants", SIAM J. Sci. Stat. Comput. 5 (1984)), which keep
/// it increasing, C1. `t` is within `xs` (the callers sample between its ends).
fn monotone(xs: &[f64], ys: &[f64], t: f64) -> Result<f64> {
    let n = xs.len();
    if n < 2 || ys.len() != n {
        return Err(bad("a reparameterisation of fewer than two points"));
    }
    // the piece holding t
    let mut i = 0;
    while i + 2 < n && xs.get(i + 1).is_some_and(|&x| t > x) {
        i += 1;
    }
    let at = |j: usize| -> Result<(f64, f64)> {
        match (xs.get(j), ys.get(j)) {
            (Some(&x), Some(&y)) => Ok((x, y)),
            _ => Err(bad("a reparameterisation's point")),
        }
    };
    let secant = |j: usize| -> Result<(f64, f64)> {
        let ((x0, y0), (x1, y1)) = (at(j)?, at(j + 1)?);
        Ok((x1 - x0, (y1 - y0) / (x1 - x0)))
    };
    // the slope at point j: the secant at the ends; inside, zero where the secants differ in
    // sign, else Fritsch-Butland's weighted harmonic mean
    let slope = |j: usize| -> Result<f64> {
        if j == 0 {
            return Ok(secant(0)?.1);
        }
        if j + 1 == n {
            return Ok(secant(j - 1)?.1);
        }
        let ((h0, d0), (h1, d1)) = (secant(j - 1)?, secant(j)?);
        if d0 * d1 <= 0.0 {
            return Ok(0.0);
        }
        Ok(3.0 * (h0 + h1) / ((2.0 * h1 + h0) / d0 + (h1 + 2.0 * h0) / d1))
    };
    let ((x0, y0), (x1, y1)) = (at(i)?, at(i + 1)?);
    let h = x1 - x0;
    let s = (t - x0) / h;
    let (m0, m1) = (slope(i)?, slope(i + 1)?);
    let (s2, s3) = (s * s, s * s * s);
    Ok((2.0 * s3 - 3.0 * s2 + 1.0) * y0
        + (s3 - 2.0 * s2 + s) * h * m0
        + (-2.0 * s3 + 3.0 * s2) * y1
        + (s3 - s2) * h * m1)
}

/// The refit's sites on the common parameters `at`: `per_span` even steps to each span between
/// two of them, each common parameter itself a site (exactly), the last included.
fn samples(at: &[f64], per_span: usize) -> Result<Vec<f64>> {
    let mut params = Vec::new();
    for w in at.windows(2) {
        if let [a, b] = w {
            let (a, b) = (*a, *b);
            for i in 0..per_span {
                #[allow(clippy::cast_precision_loss)] // at most REFIT_MOST
                let f = i as f64 / per_span as f64;
                params.push(a + (b - a) * f);
            }
        }
    }
    params.push(
        at.last()
            .copied()
            .ok_or_else(|| bad("no common parameters"))?,
    );
    Ok(params)
}

/// One family refitted so its crossings sit at the common parameters `at`: each curve `c` of
/// `curves` (its own crossing parameters `own[c]`, the ends its domain's) sampled at its
/// monotone map from `at` to its own ([`monotone`]) at the sites [`samples`] places, and
/// interpolated at the sites at its own degree with averaged knots: the crossings are
/// interpolation sites, so they land on the common parameters exactly, and every curve of the
/// family gets the same knots.
fn refit_family(
    curves: &[Curve],
    own: &[Vec<f64>],
    at: &[f64],
    per_span: usize,
) -> Result<Vec<BSplineCurve>> {
    let params = samples(at, per_span)?;
    let mut out = Vec::new();
    for (c, ys) in curves.iter().zip(own) {
        let p = c.degree();
        let knots = interp_knot_vector(&params, p)?;
        let mut points = Vec::new();
        for &t in &params {
            points.push(c.eval(monotone(at, ys, t)?, Side::Left)?);
        }
        out.push(BSplineCurve::from_gw_nurb(
            3,
            &through(&points, &params, &knots, p)?,
        ));
    }
    Ok(out)
}

/// The Gordon surface of a network whose crossings are not at its common parameters (each
/// pair crossing once, in order, the outer curves crossing at their ends): each family whose
/// crossings are off its common parameters refitted to them ([`refit_family`]), the surface
/// built from the refits ([`build`]), and each INPUT curve proved within the bound of the
/// surface ([`inputs_on`]). A span past `tol` doubles the refit's points, up to [`REFIT_MOST`]
/// a span.
/// `gap` is the refusal the network had at its common parameters, returned where it cannot be
/// refitted (an outer curve crossing short of a curve's end).
///
/// # Errors
///
/// As [`gordon`].
fn refitted(
    k: &Kernel,
    us: &[BSplineCurve],
    vs: &[BSplineCurve],
    tol: f64,
    gap: NetworkRefusal,
) -> Result<std::result::Result<Gordon, NetworkRefusal>> {
    let Network {
        cu,
        dv,
        u_cross,
        v_cross,
        u_at,
        v_at,
    } = match network(k, us, vs)? {
        Ok(n) => n,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let (Some(c0), Some(d0)) = (cu.first(), dv.first()) else {
        return Err(bad("an empty family"));
    };
    let ((ua, ub), (va, vb)) = (c0.param_bounds(), d0.param_bounds());
    // each curve's own crossing parameters, its domain's ends at the outer curves, where the
    // outer curves must cross it
    let ends_on = |c: &Curve, t: f64, end: f64| -> Result<bool> {
        Ok(within(
            (c.eval(t, Side::Left)? - c.eval(end, Side::Left)?).length(),
            tol,
        ))
    };
    let mut u_own: Vec<Vec<f64>> = Vec::new();
    for (c, row) in cu.iter().zip(&u_cross) {
        let (Some(&first), Some(&last)) = (row.first(), row.last()) else {
            return Err(bad("a curve without crossings"));
        };
        if !ends_on(c, first, ua)? || !ends_on(c, last, ub)? {
            return Ok(Err(gap));
        }
        let mut own = row.clone();
        if let Some(x) = own.first_mut() {
            *x = ua;
        }
        if let Some(x) = own.last_mut() {
            *x = ub;
        }
        if !increasing(&own) {
            return Ok(Err(NetworkRefusal::Order));
        }
        u_own.push(own);
    }
    let mut v_own: Vec<Vec<f64>> = Vec::new();
    for (l, d) in dv.iter().enumerate() {
        let column: Vec<f64> = v_cross
            .iter()
            .filter_map(|row| row.get(l).copied())
            .collect();
        let (Some(&first), Some(&last)) = (column.first(), column.last()) else {
            return Err(bad("a curve without crossings"));
        };
        if !ends_on(d, first, va)? || !ends_on(d, last, vb)? {
            return Ok(Err(gap));
        }
        let mut own = column;
        if let Some(x) = own.first_mut() {
            *x = va;
        }
        if let Some(x) = own.last_mut() {
            *x = vb;
        }
        if !increasing(&own) {
            return Ok(Err(NetworkRefusal::Order));
        }
        v_own.push(own);
    }
    // a family whose crossings are all at the common parameters is kept as it is
    let off = |own: &[Vec<f64>], at: &[f64]| own.iter().any(|row| row.as_slice() != at);
    let (u_off, v_off) = (off(&u_own, &u_at), off(&v_own, &v_at));

    let mut reached = f64::INFINITY;
    let mut per_span = REFIT_FIRST;
    while per_span <= REFIT_MOST {
        let ru = if u_off {
            refit_family(&cu, &u_own, &u_at, per_span)?
        } else {
            us.to_vec()
        };
        let rv = if v_off {
            refit_family(&dv, &v_own, &v_at, per_span)?
        } else {
            vs.to_vec()
        };
        let made = match build(k, &ru, &rv, tol)? {
            Ok(made) => made,
            Err(NetworkRefusal::PastTolerance { bound }) => {
                reached = bound;
                per_span *= 2;
                continue;
            }
            Err(refusal) => return Ok(Err(refusal)),
        };
        let bound = inputs_on(
            k,
            &made.surface,
            &cu,
            &dv,
            &u_own,
            &v_own,
            &u_at,
            &v_at,
            per_span,
            tol,
        )?;
        if within(bound, tol) {
            return Ok(Ok(Gordon {
                surface: made.surface,
                bound: bound.max(made.bound),
            }));
        }
        reached = bound;
        per_span *= 2;
    }
    Ok(Err(NetworkRefusal::PastTolerance { bound: reached }))
}

/// The largest proved distance between the surface and each input curve ([`span_bound`]), piece
/// by piece between the refit's sites ([`samples`]): u-curve `kk` at site `x` is the surface's
/// `(x, v_at[kk])` and its own parameter `monotone(u_at, u_own[kk], x)`, the point the refit
/// went through there (a family kept as it is maps each site to itself), and each v-curve
/// likewise. Nodes only at the crossings would not do: [`span_bound`] cuts a piece at each knot
/// line of the surface with the curve's parameter interpolated linearly between the nodes, and
/// the monotone map is not linear there, so each cut would slide along the curve by its
/// departure from the line, a floor no halving lowers (the refit's own knots between two
/// crossings are all such cuts).
#[allow(clippy::too_many_arguments)]
fn inputs_on(
    k: &Kernel,
    surface: &BSplineSurface,
    cu: &[Curve],
    dv: &[Curve],
    u_own: &[Vec<f64>],
    v_own: &[Vec<f64>],
    u_at: &[f64],
    v_at: &[f64],
    per_span: usize,
    tol: f64,
) -> Result<f64> {
    let surf = crate::surface::surface::Surface::from(surface.clone());
    let mut worst = 0.0_f64;
    // one family: each curve, its own crossing parameters, the common ones, and per curve and
    // site the surface's (u, v)
    let mut family = |curves: &[Curve],
                      own: &[Vec<f64>],
                      at: &[f64],
                      uv: &dyn Fn(usize, f64) -> Option<Point2>|
     -> Result<()> {
        let sites = samples(at, per_span)?;
        for (i, (c, ts)) in curves.iter().zip(own).enumerate() {
            let rail = kernel_curve(c);
            let start = c.eval(ts.first().copied().unwrap_or(0.0), Side::Left)?;
            let spans = RailSpans::about(c, start)?;
            let node = |x: f64| -> Result<Node> {
                let uv = uv(i, x).ok_or_else(|| bad("a site without its parameters"))?;
                Ok(Node {
                    t: monotone(at, ts, x)?,
                    uv,
                })
            };
            for w in sites.windows(2) {
                if let [a, b] = w {
                    let bound =
                        span_bound(k, &surf, &rail, &spans, start, node(*a)?, node(*b)?, tol)?;
                    worst = worst.max(bound);
                }
            }
        }
        Ok(())
    };
    family(cu, u_own, u_at, &|kk, x| {
        Some(Point2::new(x, *v_at.get(kk)?))
    })?;
    family(dv, v_own, v_at, &|l, x| Some(Point2::new(*u_at.get(l)?, x)))?;
    Ok(worst)
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

    /// The cubic through `points` at their chord-length parameters, with averaged knots: how an
    /// edit-point curve is fitted, so two curves through points at the same stations but at
    /// different heights are parameterised differently.
    fn ep_curve(points: &[[f64; 3]]) -> BSplineCurve {
        let pts: Vec<Vec3> = points.iter().map(|q| Vec3::new(q[0], q[1], q[2])).collect();
        let mut params = vec![0.0];
        for w in pts.windows(2) {
            if let [a, b] = w {
                params.push(params.last().copied().unwrap() + (*b - *a).length());
            }
        }
        let total = *params.last().unwrap();
        let params: Vec<f64> = params.iter().map(|t| t / total).collect();
        let knots = interp_knot_vector(&params, 3).unwrap();
        let c = through(&pts, &params, &knots, 3).unwrap();
        BSplineCurve::from_gw_nurb(3, &c)
    }

    /// A deck: a roofline and a shoulder through the same five stations along x at different
    /// heights (their chord-length parameters differ at each station), and three sections
    /// across, each ending on the two rails' own points at its station.
    fn deck() -> (Vec<BSplineCurve>, Vec<BSplineCurve>) {
        let xs = [0.0, 10.0, 25.0, 40.0, 50.0];
        let (roof, shoulder) = ([0.0, 6.0, 9.0, 7.0, 5.0], [0.0, 2.0, 1.0, 4.0, 5.0]);
        let rail = |y: f64, z: &[f64; 5]| {
            let pts: Vec<[f64; 3]> = xs.iter().zip(z).map(|(&x, &z)| [x, y, z]).collect();
            ep_curve(&pts)
        };
        let us = vec![rail(0.0, &roof), rail(10.0, &shoulder)];
        let section = |i: usize| {
            let (x, za, zb) = (xs[i], roof[i], shoulder[i]);
            spline(
                &[
                    [x, 0.0, za],
                    [x, 3.0, za + 2.0],
                    [x, 7.0, zb + 2.0],
                    [x, 10.0, zb],
                ],
                3,
            )
        };
        (us, vec![section(0), section(2), section(4)])
    }

    /// Every point of `c` at `n + 1` even steps lies within `bound` of the surface (found by the
    /// kernel's projection, independent of the network's parameters).
    fn on_surface(k: &Kernel, s: &BSplineSurface, c: &BSplineCurve, bound: f64, n: usize) {
        let surf = crate::surface::surface::Surface::from(s.clone());
        let nurb = c.nurb.as_ref().unwrap();
        let (a, b) = nurb.param_bounds();
        for i in 0..=n {
            let t = a + (b - a) * i as f64 / n as f64;
            let q = nurb.eval(t, Side::Left).unwrap();
            let d = crate::query::api_queries::surface_closest_point(k, &surf, q)
                .unwrap()
                .closest
                .distance;
            assert!(
                d <= bound + 1e-9,
                "{q:?} is {d} off the surface (bound {bound})"
            );
        }
    }

    /// The deck: main refuses it (a gap at the mean parameters, though every crossing is
    /// exact); its curves crossing once each and in order, it has a surface, and each of its five
    /// curves lies on it within the bound.
    #[test]
    fn a_deck_whose_crossings_are_not_at_common_parameters_has_a_surface() {
        let k = Kernel::default();
        let (us, vs) = deck();
        let tol = 1e-3;
        let g = gordon(&k, &us, &vs, tol)
            .unwrap()
            .unwrap_or_else(|r| panic!("refused: {r:?}"));
        assert!(g.bound <= tol, "{}", g.bound);
        for c in us.iter().chain(&vs) {
            on_surface(&k, &g.surface, c, g.bound, 20);
        }
    }

    /// The deck's refit, compared with the original rails directly: at every site the refit is
    /// the rail at its monotone map's parameter, so at each common parameter it is the rail's
    /// own crossing.
    #[test]
    fn the_decks_refit_goes_through_the_rails_at_their_crossings() {
        let k = Kernel::default();
        let (us, vs) = deck();
        let net = network(&k, &us, &vs).unwrap().unwrap();
        let (ua, ub) = net.cu[0].param_bounds();
        let own: Vec<Vec<f64>> = net
            .u_cross
            .iter()
            .map(|row| {
                let mut row = row.clone();
                *row.first_mut().unwrap() = ua;
                *row.last_mut().unwrap() = ub;
                row
            })
            .collect();
        let refits = refit_family(&net.cu, &own, &net.u_at, REFIT_FIRST).unwrap();
        let sites = samples(&net.u_at, REFIT_FIRST).unwrap();
        for ((c, ts), r) in net.cu.iter().zip(&own).zip(&refits) {
            let r = r.nurb.as_ref().unwrap();
            for &x in &sites {
                let d = (r.eval(x, Side::Left).unwrap()
                    - c.eval(monotone(&net.u_at, ts, x).unwrap(), Side::Left)
                        .unwrap())
                .length();
                assert!(d < 1e-9, "the refit at {x} is {d} off the rail");
            }
            for (&x, &t) in net.u_at.iter().zip(ts) {
                assert!(sites.contains(&x), "the common parameter {x} is not a site");
                assert_eq!(monotone(&net.u_at, ts, x).unwrap().to_bits(), t.to_bits());
            }
        }
    }

    /// A section crossing the roofline twice is refused, as before.
    #[test]
    fn a_curve_crossing_another_twice_is_still_refused() {
        let k = Kernel::default();
        let us = [
            line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
            line([0.0, 10.0, 0.0], [10.0, 10.0, 0.0]),
        ];
        // the middle section dips below y = 0 and comes back across it
        let vs = [
            line([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]),
            spline(
                &[
                    [5.0, 0.0, 0.0],
                    [5.0, -4.0, 0.0],
                    [5.0, 4.0, 0.0],
                    [5.0, 10.0, 0.0],
                ],
                3,
            ),
            line([10.0, 0.0, 0.0], [10.0, 10.0, 0.0]),
        ];
        assert!(matches!(
            gordon(&k, &us, &vs, 1e-6).unwrap().unwrap_err(),
            NetworkRefusal::Crossings { u: 0, v: 1, count } if count >= 2
        ));
    }

    /// A network crossing at its common parameters takes the compatible path as it is: the
    /// same surface (net, degrees, knots) and the same bound as building it directly.
    #[test]
    fn a_compatible_network_is_built_as_before() {
        let k = Kernel::default();
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
        let direct = build(&k, &us, &vs, 1e-6).unwrap().unwrap();
        assert_eq!(g.bound.to_bits(), direct.bound.to_bits());
        let (a, b) = (
            g.surface.nurb.as_ref().unwrap(),
            direct.surface.nurb.as_ref().unwrap(),
        );
        assert_eq!(a.degrees(), b.degrees());
        assert_eq!(a.u_knots(), b.u_knots());
        assert_eq!(a.v_knots(), b.v_knots());
        let bits = |s: &Surface| -> Vec<[u64; 4]> {
            s.rows()
                .flatten()
                .map(|c| [c.x.to_bits(), c.y.to_bits(), c.z.to_bits(), c.w.to_bits()])
                .collect()
        };
        assert_eq!(bits(a), bits(b));
    }

    #[test]
    fn the_monotone_map_goes_through_its_points_and_increases() {
        let (xs, ys) = ([0.0, 0.3, 0.7, 1.0], [0.0, 0.5, 0.6, 1.0]);
        for (x, y) in xs.iter().zip(ys) {
            assert!((monotone(&xs, &ys, *x).unwrap() - y).abs() < 1e-15);
        }
        let mut last = f64::NEG_INFINITY;
        for i in 0..=100 {
            let y = monotone(&xs, &ys, f64::from(i) / 100.0).unwrap();
            assert!(y >= last, "{y} after {last}");
            last = y;
        }
    }
}
