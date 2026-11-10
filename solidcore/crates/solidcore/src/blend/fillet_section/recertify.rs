//! A blend by its definition re-certified (#2388): its net B proved within the
//! definition's bound ε of S again, as a blend rebuilt from a record must be before it is
//! an operand. A net that is not proved within ε, or is not a net of its definition's
//! stations, is refused.
//!
//! Not upstream. The fork's `SmBallPatch::IsOnSphere` is the same check for the Ball's
//! sphere patch, which an exact algebraic identity settles; a rolling-ball round has
//! none, so the round's own certificate is run again.
//!
//! The net is the construction's (`Build`): along s, on each span between two stations, the
//! cubic Hermite interpolant of the section's rows at its stations; across, the section's
//! exact rational form. The stations are the seeds' knots (the seeds are the feet's
//! Hermite curves on them, and are never cut). So for each piece of B:
//!
//! - its span's stations are solved again from the definition, and the span's Hermite
//!   rows H built from them ([`NetForm::span`], the construction's own);
//! - `|B - H|` is bounded on the piece by its control nets: with both rational Bezier
//!   patches of the same degrees, Bernstein bases `N`, points `P_i`, `Q_i` and positive
//!   weights `w_i`, `v_i`, `B - H = sum_ij N_i N_j [w_i v_j (P_i - Q_i) + (Q_i - c)
//!   (w_i v_j - v_i w_j)] / (W V)` for any point c, so
//!   `|B - H| <= max |P_i - Q_i| + max |Q_i - c| max |w_i v_j - v_i w_j| / (min w min v)`;
//! - `|H - S|` on the span is the round's own certificate ([`gn_certify_span`]), asked for
//!   what ε leaves after that.
//!
//! A piece past the section's range `[0, 1]` (a round extended across its section,
//! [`crate::surface::extend::blend`]) is compared with H continued: its rows cut by
//! blossoms at the piece's range (the rounding of that extrapolation counted), and
//! `|H - S|` there is the certificate's bound on `[0, 1]` times the growth of
//! docs/definitions-and-certificates.md 6.4 ([`section_growth`]).
//!
//! The bound is their sum with the floor ([`precision_floor`]), a forward error bound of
//! computing H, at most ε, or the net is refused. A blend stores ε as its certificate, the
//! net's rounding off its rows and three floors: H computed again is within two floors of
//! the H the net was made of. A net cut or reparametrized
//! after it was built (`SplitAt`, `Reparameterize`, `Reverse`, `SwapUV`) is a piece of the
//! same Hermite net: H is cut to the piece, in homogeneous coordinates, before the nets
//! are compared.
//!
//! Deviations: none (no upstream counterpart). No recursion. The mapped-net certificate
//! accepts degrees at most 16, at most 262,144 retained interval coefficients, and at most
//! eight million refinement steps or coefficient comparisons.

use core::cmp::Ordering;

use super::certificate::{gn_certify_span, gn_certify_span_reusing, GnLaw, NurbsCurve, SpanReuse};
use super::continuation::{growth, section_degree, section_weight_range};
use super::interval::Ivl;
use super::section::{
    certificate_faces, crosses_knot, inner_knots, net_of_spans, param, varying, NetForm,
    StationSystem, GN_THUMBWEIGHT_EQUAL_AREA,
};
use super::series::Trunc;
use super::station::{as_b_spline, GnStation, Nurbs, STATION_STEP};
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::{gamma, NOW};
use crate::nurbs::srf_tool::p1::decompose_to_bez;
use crate::surface::procedural_blend::ProceduralBlend;
use crate::surface::surface::Surface;

const OP: &str = "blend::fillet_section::recertify";

/// A homogeneous control point `(w P, w)`.
type Hpt = (Vec3, f64);

/// A Bezier patch over the definition's (s, t): its homogeneous control points
/// `[i along s][j along t]` and its box.
struct Piece {
    net: Vec<Vec<Hpt>>,
    s: [f64; 2],
    t: [f64; 2],
}

/// The largest rounding of `a + b x` (its two operations, each within half an ulp, and
/// the operands' size).
fn map_rounding(a: f64, b: f64, x: f64) -> f64 {
    4.0 * f64::EPSILON * (a.abs() + (b * x).abs())
}

/// One Bezier curve of homogeneous points cut to `[lo, hi]` of its `[0, 1]` (de Casteljau,
/// at hi, then the left part at lo / hi).
fn sub_curve(pts: &[Hpt], lo: f64, hi: f64) -> Vec<Hpt> {
    let split = |p: &[Hpt], tau: f64| {
        split_curve(p, tau, &|a: Hpt, b: Hpt, tau| {
            (a.0 * (1.0 - tau) + b.0 * tau, a.1 * (1.0 - tau) + b.1 * tau)
        })
    };
    let (left, _) = if hi < 1.0 {
        split(pts, hi)
    } else {
        (pts.to_vec(), Vec::new())
    };
    if lo > 0.0 && hi > 0.0 {
        split(&left, lo / hi).1
    } else {
        left
    }
}

/// One Bezier curve of homogeneous points re-expressed over `[lo, hi]` of its parameter,
/// where that range may lie outside `[0, 1]`: the same rational polynomial continued. Its
/// control points are the blossom `b(lo^(d-i), hi^i)`, each by de Casteljau steps at `lo`
/// then `hi` (an affine step is valid at any parameter). Returns the points and `E`, a
/// bound of how far each of their four homogeneous coordinates is from the exact ones: a
/// step `(1 - tau) a + tau b` is four operations on coordinates of at most `M` (the input's
/// largest, `Lambda^k M` after k steps, `Lambda = max (|1 - tau| + |tau|)`), so by the
/// recurrence `e_(k+1) <= Lambda e_k + gamma_4 Lambda^(k+1) M` the d steps are within
/// `d gamma_4 Lambda^d M`.
fn extrapolate_curve(pts: &[Hpt], lo: f64, hi: f64) -> Result<(Vec<Hpt>, f64)> {
    let d = pts
        .len()
        .checked_sub(1)
        .ok_or_else(|| SolidError::sm_err(OP))?;
    let mut out = Vec::new();
    for i in 0..=d {
        let mut level: Vec<Hpt> = pts.to_vec();
        for step in 0..d {
            let tau = if step < d - i { lo } else { hi };
            level = level
                .windows(2)
                .map(|w| match w {
                    [(pa, wa), (pb, wb)] => {
                        (*pa * (1.0 - tau) + *pb * tau, wa * (1.0 - tau) + wb * tau)
                    }
                    _ => (Vec3::ZERO, 0.0),
                })
                .collect();
        }
        out.push(
            level
                .first()
                .copied()
                .ok_or_else(|| SolidError::sm_err(OP))?,
        );
    }
    let size = pts
        .iter()
        .fold(0.0_f64, |m, &(p, w)| m.max(p.length()).max(w.abs()));
    let lambda = [lo, hi]
        .iter()
        .fold(1.0_f64, |m, &tau| m.max((1.0 - tau).abs() + tau.abs()));
    let exp = i32::try_from(d).map_err(|_| SolidError::sm_err(OP))?;
    #[allow(clippy::cast_precision_loss)]
    let err = d as f64 * gamma(4) * lambda.powi(exp) * size;
    if err.is_finite() {
        Ok((out, err))
    } else {
        Err(SolidError::invalid_input(
            OP,
            "the definition's rows are not finite past the section",
        ))
    }
}

/// `|B - H|` over a piece, from the two patches' control nets (module docs).
///
/// # Errors
///
/// `InvalidInput` where the nets differ in size or a weight is not positive.
pub(crate) fn net_distance(b: &[Vec<Hpt>], h: &[Vec<Hpt>]) -> Result<f64> {
    net_distance_with::<FloatMetric>(b, h)
}

/// Cached weight-only part of the interval net inequality. The complete list
/// of weights is checked on reuse; coordinates are always checked afresh.
#[derive(Default)]
pub(crate) struct IntervalWeightCache {
    weights: Vec<(Ivl, Ivl)>,
    cross: f64,
}

/// As `interval_net_distance`, caching only the weight-pair maximum. The flag
/// reports whether all pair comparisons were reused, for the caller's work cap.
pub(crate) fn interval_net_distance_cached(
    actual: &[Vec<([Ivl; 3], Ivl)>],
    ideal: &[Vec<([Ivl; 3], Ivl)>],
    cache: &mut IntervalWeightCache,
) -> Result<(f64, bool)> {
    let width = actual.first().map_or(0, Vec::len);
    if width == 0
        || actual.len() != ideal.len()
        || !actual.iter().chain(ideal).all(|row| row.len() == width)
        || !actual.iter().chain(ideal).flatten().all(|(p, w)| {
            p.iter()
                .chain(core::iter::once(w))
                .all(|x| x.lo.is_finite() && x.hi.is_finite() && x.lo <= x.hi)
                && IntervalMetric::positive(*w)
        })
    {
        return Err(SolidError::invalid_input(
            OP,
            "invalid interval control nets",
        ));
    }
    let weights: Vec<_> = actual
        .iter()
        .flatten()
        .zip(ideal.iter().flatten())
        .map(|(a, b)| (a.1, b.1))
        .collect();
    let reused = weights == cache.weights;
    if !reused {
        let mut cross = 0.0_f64;
        for &(wi, vi) in &weights {
            for &(wj, vj) in &weights {
                cross = cross.max(IntervalMetric::cross(wi, vi, wj, vj));
            }
        }
        cache.weights = weights;
        cache.cross = cross;
    }
    Ok((
        net_distance_with_cross::<IntervalMetric>(actual, ideal, Some(cache.cross))?,
        reused,
    ))
}

// The float and interval certificates share the same control-net inequality. FloatMetric
// retains the existing blend operation order; IntervalMetric encloses every operation for
// records whose parameter maps and restrictions must also be certified.
trait NetMetric {
    type Point: Copy;
    type Weight: Copy;
    fn positive(w: Self::Weight) -> bool;
    fn lower(w: Self::Weight) -> f64;
    fn euclid(p: (Self::Point, Self::Weight)) -> Self::Point;
    fn distance(a: Self::Point, b: Self::Point) -> f64;
    fn cross(wi: Self::Weight, vi: Self::Weight, wj: Self::Weight, vj: Self::Weight) -> f64;
    fn combine(points: f64, reach: f64, cross: f64, w_min: f64, v_min: f64) -> f64;
}

struct FloatMetric;
impl NetMetric for FloatMetric {
    type Point = Vec3;
    type Weight = f64;
    fn positive(w: f64) -> bool {
        w > 0.0 && w.is_finite()
    }
    fn lower(w: f64) -> f64 {
        w
    }
    fn euclid((p, w): Hpt) -> Vec3 {
        p * (1.0 / w)
    }
    fn distance(a: Vec3, b: Vec3) -> f64 {
        (a - b).length()
    }
    fn cross(wi: f64, vi: f64, wj: f64, vj: f64) -> f64 {
        (wi * vj - vi * wj).abs()
    }
    fn combine(points: f64, reach: f64, cross: f64, w_min: f64, v_min: f64) -> f64 {
        points + reach * cross / (w_min * v_min)
    }
}

type IPoint = [Ivl; 3];
type IHpt = (IPoint, Ivl);
struct IntervalMetric;
impl NetMetric for IntervalMetric {
    type Point = IPoint;
    type Weight = Ivl;
    fn positive(w: Ivl) -> bool {
        w.lo > 0.0 && w.lo.is_finite() && w.hi.is_finite()
    }
    fn lower(w: Ivl) -> f64 {
        w.lo
    }
    fn euclid((p, w): IHpt) -> IPoint {
        p.map(|x| x * (Ivl::from(1.0) / w))
    }
    fn distance(a: IPoint, b: IPoint) -> f64 {
        a.into_iter()
            .zip(b)
            .map(|(a, b)| {
                let d = a - b;
                d * d
            })
            .fold(Ivl::from(0.0), |a, b| a + b)
            .sqrt()
            .hi
    }
    fn cross(wi: Ivl, vi: Ivl, wj: Ivl, vj: Ivl) -> f64 {
        (wi * vj - vi * wj).mag()
    }
    fn combine(points: f64, reach: f64, cross: f64, w_min: f64, v_min: f64) -> f64 {
        (Ivl::from(points)
            + Ivl::from(reach) * Ivl::from(cross) / (Ivl::from(w_min) * Ivl::from(v_min)))
        .hi
    }
}

type MetricPoint<M> = (<M as NetMetric>::Point, <M as NetMetric>::Weight);

fn net_distance_with<M: NetMetric>(
    b: &[Vec<MetricPoint<M>>],
    h: &[Vec<MetricPoint<M>>],
) -> Result<f64> {
    net_distance_with_cross::<M>(b, h, None)
}

fn net_distance_with_cross<M: NetMetric>(
    b: &[Vec<MetricPoint<M>>],
    h: &[Vec<MetricPoint<M>>],
    saved_cross: Option<f64>,
) -> Result<f64> {
    let flat = |n: &[Vec<MetricPoint<M>>]| n.iter().flatten().copied().collect::<Vec<_>>();
    let (pb, ph) = (flat(b), flat(h));
    if pb.len() != ph.len() || pb.is_empty() {
        return Err(SolidError::invalid_input(
            OP,
            "the net is not of its definition's degrees",
        ));
    }
    if !pb.iter().chain(&ph).all(|&(_, w)| M::positive(w)) {
        return Err(SolidError::invalid_input(
            OP,
            "a weight of the net is not positive",
        ));
    }
    let first = ph.first().copied().ok_or_else(|| SolidError::sm_err(OP))?;
    let c = M::euclid(first);
    let mut d_points = 0.0_f64;
    let mut reach = 0.0_f64;
    for (&x, &y) in pb.iter().zip(&ph) {
        d_points = d_points.max(M::distance(M::euclid(x), M::euclid(y)));
        reach = reach.max(M::distance(M::euclid(y), c));
    }
    let mut cross = saved_cross.unwrap_or(0.0);
    if saved_cross.is_none() {
        for (i, &(_, wi)) in pb.iter().enumerate() {
            let vi = ph
                .get(i)
                .map(|q| q.1)
                .ok_or_else(|| SolidError::sm_err(OP))?;
            for (j, &(_, wj)) in pb.iter().enumerate() {
                let vj = ph
                    .get(j)
                    .map(|q| q.1)
                    .ok_or_else(|| SolidError::sm_err(OP))?;
                cross = cross.max(M::cross(wi, vi, wj, vj));
            }
        }
    }
    let w_min = pb
        .iter()
        .map(|q| M::lower(q.1))
        .fold(f64::INFINITY, f64::min);
    let v_min = ph
        .iter()
        .map(|q| M::lower(q.1))
        .fold(f64::INFINITY, f64::min);
    let d = M::combine(d_points, reach, cross, w_min, v_min);
    if d.is_finite() {
        Ok(d)
    } else {
        Err(SolidError::invalid_input(OP, "the net is not finite"))
    }
}

// One de Casteljau triangle, used with unchanged floating arithmetic by the blend's
// sub_curve and with interval coefficients by the mapped-net certificate.
fn split_curve<P: Copy, T: Copy>(
    p: &[P],
    tau: T,
    blend: &impl Fn(P, P, T) -> P,
) -> (Vec<P>, Vec<P>) {
    let mut level = p.to_vec();
    let mut left = Vec::new();
    let mut right = Vec::new();
    while let (Some(&first), Some(&last)) = (level.first(), level.last()) {
        left.push(first);
        right.push(last);
        level = level
            .windows(2)
            .filter_map(|w| match w {
                &[a, b] => Some(blend(a, b, tau)),
                _ => None,
            })
            .collect();
    }
    right.reverse();
    (left, right)
}

/// A span whose existing control points are already its Bernstein coefficients.
#[derive(Clone, Copy)]
struct BezierSpan {
    first: usize,
    range: [f64; 2],
}

struct BezierNet {
    net: Vec<Vec<IHpt>>,
    u: Vec<BezierSpan>,
    v: Vec<BezierSpan>,
    degrees: [usize; 2],
}

// These limits bound both retained cell data and the quadratic coefficient comparison.
const MAPPED_MAX_DEGREE: usize = 16;
const MAPPED_MAX_WORK: usize = 8_000_000;
const MAPPED_MAX_COEFFICIENTS: usize = 262_144;

fn knot_runs(knots: &[f64], degree: usize, count: usize) -> Result<Vec<(f64, usize)>> {
    let bad = || {
        SolidError::invalid_input(
            OP,
            "mapped certificate requires a finite clamped spline net",
        )
    };
    if degree == 0
        || degree > MAPPED_MAX_DEGREE
        || count <= degree
        || count.checked_add(degree).and_then(|n| n.checked_add(1)) != Some(knots.len())
        || knots.iter().any(|x| !x.is_finite())
        || !knots.windows(2).all(|w| matches!(w, [a,b] if a <= b))
    {
        return Err(bad());
    }
    let mut runs = Vec::new();
    for &knot in knots {
        match runs.last_mut() {
            Some((last, n)) if *last == knot => *n += 1,
            _ => runs.push((knot, 1_usize)),
        }
    }
    if runs.len() < 2
        || runs.iter().enumerate().any(|(i, (_, n))| {
            if i == 0 || i + 1 == runs.len() {
                *n != degree + 1
            } else {
                *n == 0 || *n > degree + 1
            }
        })
    {
        return Err(bad());
    }
    Ok(runs)
}

fn bezier_spans(knots: &[f64], degree: usize, count: usize) -> Result<Vec<BezierSpan>> {
    if knot_runs(knots, degree, count)?
        .iter()
        .skip(1)
        .rev()
        .skip(1)
        .any(|(_, n)| *n < degree)
    {
        return Err(SolidError::invalid_input(
            OP,
            "mapped net is not Bezier after refinement",
        ));
    }
    Ok(knots
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| match w {
            &[a, b] if a < b && i >= degree && i < count => Some(BezierSpan {
                first: i - degree,
                range: [a, b],
            }),
            _ => None,
        })
        .collect())
}

impl BezierNet {
    fn new(net: &BSplineSurface) -> Result<Self> {
        let nurb = net.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
        let (p, q) = nurb.degrees();
        let (n, m, _, _) = nurb.array_sizes();
        let nu = n.checked_add(1).ok_or_else(|| SolidError::sm_err(OP))?;
        let nv = m.checked_add(1).ok_or_else(|| SolidError::sm_err(OP))?;
        if nu
            .checked_mul(nv)
            .is_none_or(|n| n > MAPPED_MAX_COEFFICIENTS)
        {
            return Err(SolidError::invalid_input(
                OP,
                "mapped net certificate work budget",
            ));
        }
        if nurb.rows().count() != nu {
            return Err(SolidError::invalid_input(OP, "mapped net row count"));
        }
        for row in nurb.rows() {
            if row.len() != nv {
                return Err(SolidError::sm_err(OP));
            }
            for c in row {
                let w = if c.w == NOW { 1.0 } else { c.w };
                if ![c.x, c.y, c.z, w].into_iter().all(f64::is_finite) || w <= 0.0 {
                    return Err(SolidError::invalid_input(
                        OP,
                        "mapped net has a nonfinite coefficient or nonpositive weight",
                    ));
                }
            }
        }
        let mut coefficients = nurb
            .rows()
            .map(|row| {
                row.iter()
                    .map(|c| {
                        (
                            [Ivl::from(c.x), Ivl::from(c.y), Ivl::from(c.z)],
                            Ivl::from(if c.w == NOW { 1.0 } else { c.w }),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let (mut u, mut v) = (nurb.u_knots().to_vec(), nurb.v_knots().to_vec());
        let mut work = 0;
        refine_bezier(&mut coefficients, &mut u, p, true, &mut work)?;
        refine_bezier(&mut coefficients, &mut v, q, false, &mut work)?;
        Ok(Self {
            u: bezier_spans(&u, p, coefficients.len())?,
            v: bezier_spans(&v, q, coefficients.first().map_or(0, Vec::len))?,
            net: coefficients,
            degrees: [p, q],
        })
    }
    fn cell(
        &self,
        u: BezierSpan,
        v: BezierSpan,
        swapped: bool,
        flip: [bool; 2],
    ) -> Result<Vec<Vec<IHpt>>> {
        let [p, q] = self.degrees;
        let [ds, dt] = if swapped { [q, p] } else { [p, q] };
        let mut net = vec![vec![([Ivl::from(0.0); 3], Ivl::from(0.0)); dt + 1]; ds + 1];
        for i in 0..=p {
            for j in 0..=q {
                let point = self
                    .net
                    .get(u.first + i)
                    .and_then(|row| row.get(v.first + j))
                    .copied()
                    .ok_or_else(|| SolidError::sm_err(OP))?;
                let (s, t) = if swapped { (j, i) } else { (i, j) };
                let s = if flip[0] { ds - s } else { s };
                let t = if flip[1] { dt - t } else { t };
                *net.get_mut(s)
                    .and_then(|r| r.get_mut(t))
                    .ok_or_else(|| SolidError::sm_err(OP))? = point;
            }
        }
        Ok(net)
    }
}

fn interval_blend((a, wa): IHpt, (b, wb): IHpt, t: Ivl) -> IHpt {
    let point = std::array::from_fn(|i| {
        let a = a.get(i).copied().unwrap_or_else(Ivl::whole);
        let b = b.get(i).copied().unwrap_or_else(Ivl::whole);
        a * (Ivl::from(1.0) - t) + b * t
    });
    (point, wa * (Ivl::from(1.0) - t) + wb * t)
}

// The standard one-knot insertion recurrence (Piegl and Tiller A5.1), evaluated
// on interval homogeneous coefficients. Only the certificate changes representation;
// neither of the model's nets is altered.
fn insert_interval_knot(
    points: &[IHpt],
    knots: &[f64],
    degree: usize,
    knot: f64,
    k: usize,
    mult: usize,
) -> Result<Vec<IHpt>> {
    let unchanged = k
        .checked_sub(degree)
        .ok_or_else(|| SolidError::sm_err(OP))?;
    (0..=points.len())
        .map(|i| {
            let at = |i| points.get(i).copied().ok_or_else(|| SolidError::sm_err(OP));
            if i <= unchanged {
                return at(i);
            }
            if i > k - mult {
                return at(i - 1);
            }
            let lo = knots
                .get(i)
                .copied()
                .ok_or_else(|| SolidError::sm_err(OP))?;
            let hi = knots
                .get(i + degree)
                .copied()
                .ok_or_else(|| SolidError::sm_err(OP))?;
            let alpha = (Ivl::from(knot) - Ivl::from(lo)) / (Ivl::from(hi) - Ivl::from(lo));
            if !alpha.lo.is_finite() || !alpha.hi.is_finite() {
                return Err(SolidError::invalid_input(
                    OP,
                    "uncertifiable knot insertion coefficient",
                ));
            }
            Ok(interval_blend(at(i - 1)?, at(i)?, alpha))
        })
        .collect()
}

fn refine_bezier(
    net: &mut Vec<Vec<IHpt>>,
    knots: &mut Vec<f64>,
    degree: usize,
    u: bool,
    work: &mut usize,
) -> Result<()> {
    let rows = net.len();
    let columns = net.first().map_or(0, Vec::len);
    let count = if u { rows } else { columns };
    let runs = knot_runs(knots, degree, count)?;
    for &(knot, mult) in runs.iter().skip(1).rev().skip(1).rev() {
        for inserted in mult..degree {
            let rows = net.len();
            let columns = net.first().map_or(0, Vec::len);
            let next = if u {
                rows.checked_add(1).and_then(|n| n.checked_mul(columns))
            } else {
                columns.checked_add(1).and_then(|n| n.checked_mul(rows))
            };
            let next = next
                .filter(|n| *n <= MAPPED_MAX_COEFFICIENTS)
                .ok_or_else(|| SolidError::invalid_input(OP, "mapped net coefficient budget"))?;
            *work = work
                .checked_add(next)
                .ok_or_else(|| SolidError::sm_err(OP))?;
            if *work > MAPPED_MAX_WORK {
                return Err(SolidError::invalid_input(
                    OP,
                    "mapped net refinement work budget",
                ));
            }
            let k = knots
                .partition_point(|x| *x <= knot)
                .checked_sub(1)
                .ok_or_else(|| SolidError::sm_err(OP))?;
            let mut refined = Vec::new();
            if u {
                let mut cols = Vec::new();
                for j in 0..columns {
                    let column = net
                        .iter()
                        .map(|row| row.get(j).copied().ok_or_else(|| SolidError::sm_err(OP)))
                        .collect::<Result<Vec<_>>>()?;
                    cols.push(insert_interval_knot(
                        &column, knots, degree, knot, k, inserted,
                    )?);
                }
                for i in 0..=rows {
                    refined.push(
                        cols.iter()
                            .map(|col| col.get(i).copied().ok_or_else(|| SolidError::sm_err(OP)))
                            .collect::<Result<Vec<_>>>()?,
                    );
                }
            } else {
                for row in net.iter() {
                    refined.push(insert_interval_knot(row, knots, degree, knot, k, inserted)?);
                }
            }
            *net = refined;
            knots.insert(k + 1, knot);
        }
    }
    Ok(())
}

fn interval_sub_curve(pts: &[IHpt], lo: Ivl, hi: Ivl) -> Vec<IHpt> {
    // Cut first at the endpoint whose normalization denominator is away from zero.
    // Exterior rounding strips can have width below an ulp at zero; cutting at hi
    // and then dividing lo/hi would discard all information there.
    if hi.lo < 0.5 {
        let right = if lo == Ivl::from(0.0) {
            pts.to_vec()
        } else {
            split_curve(pts, lo, &interval_blend).1
        };
        let t = if lo == Ivl::from(0.0) {
            hi
        } else {
            (hi - lo) / (Ivl::from(1.0) - lo)
        };
        return split_curve(&right, t, &interval_blend).0;
    }
    let left = if hi == Ivl::from(1.0) {
        pts.to_vec()
    } else {
        split_curve(pts, hi, &interval_blend).0
    };
    if lo == Ivl::from(0.0) {
        left
    } else {
        split_curve(&left, lo / hi, &interval_blend).1
    }
}

fn interval_patch(net: &[Vec<IHpt>], s: [Ivl; 2], t: [Ivl; 2]) -> Result<Vec<Vec<IHpt>>> {
    let columns = net.first().map_or(0, Vec::len);
    let mut cut_columns = Vec::new();
    for j in 0..columns {
        let column = net
            .iter()
            .map(|r| r.get(j).copied().ok_or_else(|| SolidError::sm_err(OP)))
            .collect::<Result<Vec<_>>>()?;
        cut_columns.push(interval_sub_curve(&column, s[0], s[1]));
    }
    (0..net.len())
        .map(|i| {
            let row = cut_columns
                .iter()
                .map(|c| c.get(i).copied().ok_or_else(|| SolidError::sm_err(OP)))
                .collect::<Result<Vec<_>>>()?;
            Ok(interval_sub_curve(&row, t[0], t[1]))
        })
        .collect()
}

// An exactly representable affine endpoint is kept exact (including identity and reversal).
// Otherwise interval arithmetic encloses both operations. A power-of-two scale with a
// normal result is exact. Above 2^-511, a product's at-most-106-bit significand also
// guarantees its FMA residual cannot underflow, so a zero residual proves exactness.
fn affine_endpoint(a: f64, b: f64, x: f64) -> Ivl {
    let product = b * x;
    let sum = a + product;
    let power_of_two = b.is_normal() && b.abs().to_bits() & ((1_u64 << 52) - 1) == 0;
    let exact_product = b == 0.0
        || x == 0.0
        || b == 1.0
        || b == -1.0
        || (power_of_two && product.is_normal())
        || (product.abs() >= f64::from_bits(512_u64 << 52) && b.mul_add(x, -product) == 0.0);
    if product.is_finite() && sum.is_finite() && exact_product {
        let z = sum - a;
        if (a - (sum - z)) + (product - z) == 0.0 {
            return Ivl::from(sum);
        }
    }
    Ivl::from(a) + Ivl::from(b) * Ivl::from(x)
}

fn mapped_range(range: [f64; 2], a: f64, b: f64) -> [Ivl; 2] {
    let endpoints = range.map(|x| affine_endpoint(a, b, x));
    if b < 0.0 {
        [endpoints[1], endpoints[0]]
    } else {
        endpoints
    }
}

fn normalized_range(range: [f64; 2], domain: [Ivl; 2]) -> [Ivl; 2] {
    range.map(|x| (Ivl::from(x) - domain[0]) / (domain[1] - domain[0]))
}

#[derive(Clone, Copy)]
struct CoveredSpan {
    source: BezierSpan,
    range: [f64; 2],
    endpoint: Option<f64>,
}

impl CoveredSpan {
    fn parameters(self) -> [Ivl; 2] {
        match self.endpoint {
            Some(t) => [Ivl::from(t); 2],
            None => normalized_range(self.range, self.source.range.map(Ivl::from)),
        }
    }
}

// A clamped surface is piecewise rational on the interior cells and constant along
// each exterior direction. Explicit boundary strips cover every mapped parameter,
// including outward rounding bands; no part of the actual net is discarded.
fn covered_spans(spans: &[BezierSpan], range: [Ivl; 2], clamped: bool) -> Result<Vec<CoveredSpan>> {
    let first = *spans.first().ok_or_else(|| SolidError::sm_err(OP))?;
    let last = *spans.last().ok_or_else(|| SolidError::sm_err(OP))?;
    let [lo, hi] = [range[0].lo, range[1].hi];
    let mut result = Vec::new();
    if clamped && lo < first.range[0] {
        result.push(CoveredSpan {
            source: first,
            range: [lo, hi.min(first.range[0])],
            endpoint: Some(0.0),
        });
    }
    let start = spans.partition_point(|p| p.range[1] <= lo);
    for &source in spans.iter().skip(start).take_while(|p| p.range[0] < hi) {
        let range = [lo.max(source.range[0]), hi.min(source.range[1])];
        if range[0] < range[1] {
            result.push(CoveredSpan {
                source,
                range,
                endpoint: None,
            });
        }
    }
    if clamped && hi > last.range[1] {
        result.push(CoveredSpan {
            source: last,
            range: [lo.max(last.range[1]), hi],
            endpoint: Some(1.0),
        });
    }
    Ok(result)
}

/// Certifies against the expected net evaluated with its natural-domain clamping.
///
/// Exterior mapped strips are compared to constant endpoint Bernstein rows/columns,
/// including both exterior directions at a corner. This matches the sweep definition's
/// clamped profile and turn evaluation without allowing unexamined boundary slivers.
///
/// # Errors
/// Those of [`mapped_net_distance`], except that an exterior mapped strip is certified
/// rather than refused. The same degree, coefficient and work budgets apply.
pub(crate) fn mapped_clamped_net_distance(
    actual: &BSplineSurface,
    expected: &BSplineSurface,
    a: [f64; 2],
    b: [f64; 2],
    swapped: bool,
) -> Result<f64> {
    let ad = actual.natural_uv_domain();
    let ed = expected.natural_uv_domain();
    let [u, v] = if swapped {
        [[ad.v_min(), ad.v_max()], [ad.u_min(), ad.u_max()]]
    } else {
        [[ad.u_min(), ad.u_max()], [ad.v_min(), ad.v_max()]]
    };
    let u = mapped_range(u, a[0], b[0]);
    let v = mapped_range(v, a[1], b[1]);
    if u[0].lo >= ed.u_min()
        && u[1].hi <= ed.u_max()
        && v[0].lo >= ed.v_min()
        && v[1].hi <= ed.v_max()
    {
        mapped_net_distance(actual, expected, a, b, swapped)
    } else {
        mapped_net_distance_with(actual, expected, a, b, swapped, true)
    }
}

/// Encloses the distance between an actual net and an expected net under its definition map.
///
/// `(s,t)=(a[0]+b[0]*p,a[1]+b[1]*q)`, with `(p,q)=(u,v)` or `(v,u)`
/// when swapped. Homogeneous coefficients are refined to Bezier cells using interval knot
/// insertion; interval de Casteljau cuts then enclose both patches on each common cell.
/// Neither input net is changed, and no floating decomposition is trusted.
///
/// # Errors
///
/// Refuses malformed knots, nonpositive weights, differing mapped degrees, singular maps,
/// mapped domains not proved inside the expected net, degrees above 16, more than 262,144
/// interval coefficients, or eight million refinement steps or coefficient comparisons.
/// No input-driven recursion is used.
pub(crate) fn mapped_net_distance(
    actual: &BSplineSurface,
    expected: &BSplineSurface,
    a: [f64; 2],
    b: [f64; 2],
    swapped: bool,
) -> Result<f64> {
    mapped_net_distance_with(actual, expected, a, b, swapped, false)
}

fn mapped_net_distance_with(
    actual: &BSplineSurface,
    expected: &BSplineSurface,
    a: [f64; 2],
    b: [f64; 2],
    swapped: bool,
    clamped: bool,
) -> Result<f64> {
    let bad = |s| SolidError::invalid_input(OP, s);
    if !a.into_iter().chain(b).all(f64::is_finite) || b.into_iter().any(|x| x == 0.0) {
        return Err(bad(
            "mapped net certificate requires a finite nonsingular map",
        ));
    }
    let actual = BezierNet::new(actual)?;
    let expected = BezierNet::new(expected)?;
    let degrees = if swapped {
        [actual.degrees[1], actual.degrees[0]]
    } else {
        actual.degrees
    };
    if degrees != expected.degrees {
        return Err(bad("mapped net certificate requires equal degrees"));
    }
    let expected_domain = |spans: &[BezierSpan]| -> Result<[f64; 2]> {
        Ok([
            spans.first().ok_or_else(|| bad("empty mapped net"))?.range[0],
            spans.last().ok_or_else(|| bad("empty mapped net"))?.range[1],
        ])
    };
    let es = expected_domain(&expected.u)?;
    let et = expected_domain(&expected.v)?;
    let mut work = 0_usize;
    let mut bound = 0.0_f64;
    for &u in &actual.u {
        for &v in &actual.v {
            let [s, t] = if swapped {
                [v.range, u.range]
            } else {
                [u.range, v.range]
            };
            let s = mapped_range(s, a[0], b[0]);
            let t = mapped_range(t, a[1], b[1]);
            if !(s[0].hi < s[1].lo
                && t[0].hi < t[1].lo
                && [s[0].lo, s[1].hi, t[0].lo, t[1].hi]
                    .into_iter()
                    .all(f64::is_finite))
                || (!clamped
                    && (s[0].lo < es[0] || s[1].hi > es[1] || t[0].lo < et[0] || t[1].hi > et[1]))
            {
                return Err(bad(
                    "mapped net domain is not proved inside its expected net",
                ));
            }
            let actual_cell = actual.cell(u, v, swapped, [b[0] < 0.0, b[1] < 0.0])?;
            let s_spans = covered_spans(&expected.u, s, clamped)?;
            let t_spans = covered_spans(&expected.v, t, clamped)?;
            for &eu in &s_spans {
                for &ev in &t_spans {
                    let sr = eu.range;
                    let tr = ev.range;
                    let coefficients = (degrees[0] + 1) * (degrees[1] + 1);
                    work = work
                        .checked_add(coefficients * coefficients)
                        .ok_or_else(|| bad("mapped net certificate work budget"))?;
                    if work > MAPPED_MAX_WORK {
                        return Err(bad("mapped net certificate work budget"));
                    }
                    let ac = interval_patch(
                        &actual_cell,
                        normalized_range(sr, s),
                        normalized_range(tr, t),
                    )?;
                    let ec = expected.cell(eu.source, ev.source, false, [false, false])?;
                    let ec = interval_patch(&ec, eu.parameters(), ev.parameters())?;
                    bound = bound.max(net_distance_with::<IntervalMetric>(&ac, &ec)?);
                }
            }
        }
    }
    Ok(bound)
}

/// A NURBS surface's derivative bounds `(max |S_u|, max |S_v|)`: degree times the largest
/// control point difference over its knot span, times the square of the weights' ratio
/// for a rational surface (the rational derivative's bound, Farin).
fn derivative_bounds(n: &Nurbs) -> (f64, f64) {
    let at = |i: usize, j: usize| -> (Vec3, f64) {
        let q = 4 * (i * n.nv + j);
        let c = |o: usize| n.pw.get(q + o).copied().unwrap_or(f64::NAN);
        let w = c(3);
        (Vec3::new(c(0), c(1), c(2)) * (1.0 / w), w)
    };
    let (mut w_lo, mut w_hi) = (f64::INFINITY, 0.0_f64);
    for i in 0..n.nu {
        for j in 0..n.nv {
            let w = at(i, j).1;
            w_lo = w_lo.min(w);
            w_hi = w_hi.max(w);
        }
    }
    let ratio = (w_hi / w_lo) * (w_hi / w_lo);
    let knot = |kv: &[f64], i: usize| kv.get(i).copied().unwrap_or(f64::NAN);
    #[allow(clippy::cast_precision_loss)]
    let (p, q) = (n.p as f64, n.q as f64);
    let (mut lu, mut lv) = (0.0_f64, 0.0_f64);
    for i in 0..n.nu {
        for j in 0..n.nv {
            if i + 1 < n.nu {
                let span = knot(&n.u, i + n.p + 1) - knot(&n.u, i + 1);
                if span > 0.0 {
                    lu = lu.max(p * (at(i + 1, j).0 - at(i, j).0).length() / span);
                }
            }
            if j + 1 < n.nv {
                let span = knot(&n.v, j + n.q + 1) - knot(&n.v, j + 1);
                if span > 0.0 {
                    lv = lv.max(q * (at(i, j + 1).0 - at(i, j).0).length() / span);
                }
            }
        }
    }
    (lu * ratio, lv * ratio)
}

/// The floor of a blend's bound: a forward error bound of how S and its net are computed,
/// `E = G (D + gamma_K M)`:
///
/// - D, the stations: Newton stops after a step of at most `STATION_STEP (1 + |x|)` in each
///   foot parameter ([`STATION_STEP`]), which bounds the feet's error in the parameters;
///   in space a foot is then within `L_u eta_u + L_v eta_v` of its root, `L` the support's
///   derivative bounds and `eta = STATION_STEP (1 + max |knot|)`, the worse support; the
///   jets and the centre are smooth functions of the feet and move with them to first
///   order;
/// - `gamma_K M`, the rows: each control point of the net is a sum of at most `2 (n + 1)`
///   products of the section's coefficients with the jets (or the arc's rows), then a
///   Hermite point (two operations), its division by its weight and the weight's scale (two
///   more): `K = 2 (n + 1) + 6` operations on coordinates of at most M, the net's largest
///   coordinate;
/// - G, the rows' weights: a homogeneous error becomes a Euclidean one divided by the
///   weight, at most the net's weight ratio `w_max / w_min` times its size.
///
/// It does not depend on where the round lies. At part scale (a round and knots of tens of
/// millimetres) it is about 1e-10, the stations' term; the rows' is about 1e-14.
///
/// # Errors
///
/// `SM_ERR` for a support or net without NURBS data; those of [`Nurbs::of`].
pub fn precision_floor(
    k: &Kernel,
    supports: [&Surface; 2],
    order: u32,
    net: &BSplineSurface,
) -> Result<f64> {
    // (in the round's own frame: M its size about its centre)
    let (points, weights, centre) = net_points(net)?;
    let m = points
        .iter()
        .fold(0.0_f64, |a, p| a.max((*p - centre).length()));
    let (w_lo, w_hi) = weights
        .iter()
        .fold((f64::INFINITY, 0.0_f64), |(lo, hi), &w| {
            (lo.min(w), hi.max(w))
        });
    let g = w_hi / w_lo;
    let mut d = 0.0_f64;
    for s in supports {
        let b = as_b_spline(k, s)?.ok_or_else(|| SolidError::sm_err(OP))?;
        let nf = Nurbs::of(&b)?;
        let (lu, lv) = derivative_bounds(&nf);
        let reach = |kv: &[f64]| kv.iter().fold(0.0_f64, |a, &x| a.max(x.abs()));
        let (eu, ev) = (
            STATION_STEP * (1.0 + reach(&nf.u)),
            STATION_STEP * (1.0 + reach(&nf.v)),
        );
        d = d.max(lu * eu + lv * ev);
    }
    let n = order as usize;
    let floor = g * (d + gamma(2 * (n + 1) + 6) * m);
    if floor.is_finite() {
        Ok(floor)
    } else {
        Err(SolidError::sm_err(OP))
    }
}

/// A net's Euclidean control points, their weights, and the centre of their box: the
/// reference point of the round's own frame.
fn net_points(net: &BSplineSurface) -> Result<(Vec<Vec3>, Vec<f64>, Vec3)> {
    let (_, _, stored) = net.control_points()?;
    let (mut lo, mut hi) = (
        Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for c in &stored {
        let w = if c.w == NOW { 1.0 } else { c.w };
        let p = Vec3::new(c.x, c.y, c.z) * (1.0 / w);
        lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        points.push(p);
        weights.push(w);
    }
    Ok((points, weights, (lo + hi) * 0.5))
}

/// The reference point of a blend's own frame: the centre of its net's control points'
/// box. Its certificate, its net's distance from its rows and its floor are taken in
/// coordinates about it, so they follow the round's size, not where it lies.
///
/// # Errors
///
/// `SM_ERR` without NURB data.
pub(crate) fn frame_centre(net: &BSplineSurface) -> Result<Vec3> {
    Ok(net_points(net)?.2)
}

/// The rounding of a rigid motion with a uniform scale applied to a net: each homogeneous
/// coordinate is a sum of three products and the weighted translation (six operations),
/// on coordinates of at most the moved net's size, divided by the least weight.
///
/// # Errors
///
/// `SM_ERR` without NURB data.
pub(crate) fn transform_rounding(net: &BSplineSurface) -> Result<f64> {
    let (points, weights, _) = net_points(net)?;
    let m = points.iter().fold(0.0_f64, |a, p| a.max(p.length()));
    let (w_lo, w_hi) = weights
        .iter()
        .fold((f64::INFINITY, 0.0_f64), |(lo, hi), &w| {
            (lo.min(w), hi.max(w))
        });
    Ok(gamma(6) * (w_hi / w_lo) * m)
}

/// The rounding of moving a blend to its own frame: subtracting the centre rounds each
/// coordinate once, by at most `u |x - c|`, in the net and in the supports, so the net
/// and S move by at most `gamma_1` times the net's size about the centre, divided by the
/// least weight: twice that between them.
fn frame_rounding(net: &BSplineSurface) -> Result<f64> {
    let (points, weights, centre) = net_points(net)?;
    let m = points
        .iter()
        .fold(0.0_f64, |a, p| a.max((*p - centre).length()));
    let (w_lo, w_hi) = weights
        .iter()
        .fold((f64::INFINITY, 0.0_f64), |(lo, hi), &w| {
            (lo.min(w), hi.max(w))
        });
    Ok(2.0 * gamma(1) * (w_hi / w_lo) * m)
}

/// A span's Hermite rows ([`NetForm::span`]) as a grid `[i along s][j across]`.
fn span_grid(homog: &[Vec3], w: &[f64], ld: usize) -> Vec<Vec<Hpt>> {
    (0..4)
        .map(|i| {
            (0..ld)
                .map(|j| {
                    let at = i * ld + j;
                    (
                        homog.get(at).copied().unwrap_or(Vec3::ZERO),
                        w.get(at).copied().unwrap_or(f64::NAN),
                    )
                })
                .collect()
        })
        .collect()
}

/// How far the net `Build` stores is from the Hermite rows it was made of, span by span
/// (`spans`, each [`NetForm::span`]'s): its control points are the rows divided by their
/// weights and its weights scaled, each rounded, so the net is off the rows by those
/// roundings, which the definition's bound takes in.
///
/// # Errors
///
/// `SM_ERR` where the net is not the spans' (its size); those of [`net_distance`].
pub(crate) fn stored_net_distance(
    net: &BSplineSurface,
    spans: &[(Vec<Vec3>, Vec<f64>)],
    ld: usize,
) -> Result<f64> {
    let (nu, nv, stored) = net.control_points()?;
    if nv != ld || nu != 3 * spans.len() + 1 {
        return Err(SolidError::sm_err(OP));
    }
    let mut d = 0.0_f64;
    for (k, (h, w)) in spans.iter().enumerate() {
        let b: Vec<Vec<Hpt>> = (0..4)
            .map(|i| {
                (0..ld)
                    .map(|j| {
                        stored
                            .get((3 * k + i) * ld + j)
                            .map_or((Vec3::ZERO, f64::NAN), |c| {
                                (Vec3::new(c.x, c.y, c.z), if c.w == NOW { 1.0 } else { c.w })
                            })
                    })
                    .collect()
            })
            .collect();
        d = d.max(net_distance(&b, &span_grid(h, w, ld))?);
    }
    Ok(d)
}

/// The net's Bezier pieces over the definition's (s, t), each net ordered by increasing s
/// and t.
fn pieces(pb: &ProceduralBlend, ld: usize) -> Result<Vec<Piece>> {
    let not_its = || SolidError::invalid_input(OP, "the net is not of its definition's degrees");
    let d = pb.definition();
    let nurb = pb.net().gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    let patches = decompose_to_bez(nurb)?;
    let mut out = Vec::new();
    for row in &patches {
        for patch in row {
            let (p, q) = patch.degrees();
            // (p, q) along (u, v); s is u unless the map is swapped
            let (ds, dt) = if d.swapped { (q, p) } else { (p, q) };
            if ds != 3 || dt + 1 != ld {
                return Err(not_its());
            }
            let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) = (
                patch.u_knots().first(),
                patch.u_knots().last(),
                patch.v_knots().first(),
                patch.v_knots().last(),
            ) else {
                return Err(SolidError::sm_err(OP));
            };
            let (ps, pt) = if d.swapped {
                ([v0, v1], [u0, u1])
            } else {
                ([u0, u1], [v0, v1])
            };
            let ([a0, a1], [b0, b1]) = (d.map_a, d.map_b);
            let mut s = [a0 + b0 * ps[0], a0 + b0 * ps[1]];
            let mut t = [a1 + b1 * pt[0], a1 + b1 * pt[1]];
            let flip_s = d.map_b[0] < 0.0;
            let flip_t = d.map_b[1] < 0.0;
            if flip_s {
                s.swap(0, 1);
            }
            if flip_t {
                t.swap(0, 1);
            }
            // [i along s][j along t]
            let mut net = vec![vec![(Vec3::ZERO, 0.0); dt + 1]; ds + 1];
            for (iu, prow) in patch.rows().enumerate() {
                for (iv, c) in prow.iter().enumerate() {
                    let (is, it) = if d.swapped { (iv, iu) } else { (iu, iv) };
                    let is = if flip_s { ds - is } else { is };
                    let it = if flip_t { dt - it } else { it };
                    // (stored weighted when rational: homogeneous either way)
                    let w = if c.w == NOW { 1.0 } else { c.w };
                    if let Some(x) = net.get_mut(is).and_then(|r| r.get_mut(it)) {
                        *x = (Vec3::new(c.x, c.y, c.z), w);
                    }
                }
            }
            out.push(Piece { net, s, t });
        }
    }
    Ok(out)
}

/// [`ProceduralBlend::recertify`]: the net's certified distance from the definition, at
/// most the definition's bound.
///
/// # Errors
///
/// `InvalidInput` where the net is not proved within the bound, or is not a net of its
/// definition's stations (its degrees, a piece across a station or past the seeds, a piece
/// past the section whose rows' weights past it are not positive or whose section's weight vanishes, the
/// seeds' stations differing, a foot crossing a face's knot line inside a span);
/// `SM_ERR` for a support that is not a NURBS surface or is itself a blend by its
/// definition (its definition is not enclosed yet); those of the station solves and the
/// certificate.
///
/// The work is done in the round's own frame ([`frame_centre`]): the blend is moved by
/// minus its centre (supports, leader and net), so the certificate's enclosures, the
/// net's distance from its rows and the floor follow the round's size, not where it lies;
/// the move's own rounding ([`frame_rounding`]) is added to the net's distance.
pub fn recertify(k: &Kernel, pb: &ProceduralBlend) -> Result<f64> {
    Ok(recertify_with_thumbweight(k, pb)?.0)
}

/// [`recertify`], with the equal area thumbweight's proved enclosure `[lo, hi]` over every
/// span recertified ([`GnVarying::w`](super::certificate::GnVarying::w), from the certificate's interval Newton roots); `None`
/// for any other section (a constant thumbweight is its own).
///
/// # Errors
///
/// Those of [`recertify`].
pub fn recertify_with_thumbweight(
    k: &Kernel,
    pb: &ProceduralBlend,
) -> Result<(f64, Option<[f64; 2]>)> {
    crate::profile::profile_scope!(k, "blend::certificate::recertify");
    let centre = frame_centre(pb.net())?;
    let local = pb.translated(k, centre * -1.0)?;
    recertify_in_frame(k, &local, pb.bound(), frame_rounding(pb.net())?)
}

/// [`recertify_with_thumbweight`] of a blend already in its own frame, against `bound`,
/// its net's distance from its rows taken as at least `shift` more.
#[allow(clippy::too_many_lines)]
fn recertify_in_frame(
    k: &Kernel,
    pb: &ProceduralBlend,
    bound: f64,
    shift: f64,
) -> Result<(f64, Option<[f64; 2]>)> {
    let d = pb.definition();
    let n = d.order as usize;
    let equal_area = n >= 2 && d.thumbweight == GN_THUMBWEIGHT_EQUAL_AREA;
    let net_form = NetForm {
        n,
        thumbweight: d.thumbweight,
        equal_area,
        law: d.law.is_some(),
    };
    let ld = net_form.width();
    let [s0, s1] = pb.supports();
    let faces = [s0, s1];
    let ([n0, n1], mut defs) = certificate_faces(k, faces, n, d.law.is_some())?;
    let any_defs = defs.faces.iter().any(Option::is_some);
    let face_n = [&n0, &n1];
    let leader_n = NurbsCurve::of(&d.leader)?;
    let law = match &d.law {
        Some(c) => Some(GnLaw::of(c)?),
        None => None,
    };

    // the stations: the seeds' knots
    let [seed0, seed1] = &d.seeds;
    let sigma = seed0.knots(None)?.0;
    if seed1.knots(None)?.0 != sigma || sigma.len() < 2 {
        return Err(SolidError::invalid_input(
            OP,
            "the seeds are not on one set of stations",
        ));
    }
    let r = match &law {
        Some(l) => {
            let first = sigma.first().copied().unwrap_or(f64::NAN);
            let r0 = l.curve.evaluate_point(first)?.x;
            [r0, r0]
        }
        None => d.offsets,
    };
    let tr = Trunc::new(
        if law.is_some() { 2 } else { 1 },
        if n >= 2 { n } else { 0 },
    )?;
    let system = StationSystem {
        faces,
        leader: &d.leader,
        r,
        side: d.sides,
        tr,
        n,
        equal_area,
        law: law.as_ref(),
    };

    // each piece's span
    let pieces = pieces(pb, ld)?;
    let floor = precision_floor(k, faces, d.order, pb.net())?;
    let mut span_of = Vec::new();
    let mut used = vec![false; sigma.len()];
    for piece in &pieces {
        let mid = 0.5 * (piece.s[0] + piece.s[1]);
        let span = sigma
            .windows(2)
            .position(|w| matches!(w, [a, b] if *a <= mid && mid <= *b))
            .ok_or_else(|| SolidError::invalid_input(OP, "a piece of the net is past its seeds"))?;
        let (Some(&a), Some(&b)) = (sigma.get(span), sigma.get(span + 1)) else {
            return Err(SolidError::sm_err(OP));
        };
        let room_s = |x: f64| map_rounding(d.map_a[0], d.map_b[0], (x - d.map_a[0]) / d.map_b[0]);
        let along = piece.s[0] >= a - room_s(a) && piece.s[1] <= b + room_s(b);
        if !along {
            return Err(SolidError::invalid_input(
                OP,
                "a piece of the net runs across a station of its definition",
            ));
        }
        // (across, a piece may lie past the section's [0, 1]: the continued section's
        // growth covers it below; only an unbounded range is refused here)
        if !(piece.t[0].is_finite() && piece.t[1].is_finite() && piece.t[0] < piece.t[1]) {
            return Err(SolidError::invalid_input(
                OP,
                "a piece of the net has no range across the section",
            ));
        }
        span_of.push(span);
        for i in [span, span + 1] {
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
        }
    }

    // the stations the pieces need, on the pieces after them and (but the first) before
    let stations = definition_stations(k, &system, [seed0, seed1], &sigma, &used)?;
    let solved: Vec<GnStation> = stations.iter().flatten().copied().collect();
    let mut vary = varying(n, r, &solved, equal_area);

    // span by span: the nets compared, then the certificate for what the bound leaves
    let mut achieved = 0.0_f64;
    let spans: Vec<usize> = {
        let mut s = span_of.clone();
        s.sort_unstable();
        s.dedup();
        s
    };
    for span in spans {
        k.check_cancelled(OP)?;
        let (Some(Some(a)), Some(Some(b))) = (stations.get(span), stations.get(span + 1)) else {
            return Err(SolidError::sm_err(OP));
        };
        // a foot crossing a face's knot line inside the span: the construction puts a
        // station there, so this net is not its
        refuse_knot_crossing(face_n, a, b)?;
        let (ra, rb) = (net_form.station_rows(a)?, net_form.station_rows(b)?);
        let (homog, w) = net_form.span(a, &ra, b, &rb);
        let h = b.s - a.s;
        // the net's distance from the rows, over the pieces in the section's range and over
        // those past it (a different bound each)
        let mut delta_in: Option<f64> = None;
        let mut delta_out: Option<f64> = None;
        // past the section's range [0, 1], where it has pieces: the continued section's
        // growth over the rows (module docs); `reach` the farthest |2 t - 1| and `v_lo` the
        // least weight of the rows there, `across` the t range of those pieces
        let mut reach = 0.0_f64;
        let mut v_lo = f64::INFINITY;
        let mut across: Option<[f64; 2]> = None;
        let room_lo = -map_rounding(d.map_a[1], d.map_b[1], (0.0 - d.map_a[1]) / d.map_b[1]);
        let room_hi = 1.0 + map_rounding(d.map_a[1], d.map_b[1], (1.0 - d.map_a[1]) / d.map_b[1]);
        for (piece, _) in pieces.iter().zip(&span_of).filter(|(_, &sp)| sp == span) {
            // H on the piece: cut along s, then across
            let mut cut: Vec<Vec<Hpt>> = Vec::new();
            let lo = ((piece.s[0] - a.s) / h).max(0.0);
            let hi = ((piece.s[1] - a.s) / h).min(1.0);
            let grid = span_grid(&homog, &w, ld);
            let mut columns: Vec<Vec<Hpt>> = Vec::new();
            for j in 0..ld {
                let col: Vec<Hpt> = grid
                    .iter()
                    .map(|row| row.get(j).copied().unwrap_or((Vec3::ZERO, f64::NAN)))
                    .collect();
                columns.push(sub_curve(&col, lo, hi));
            }
            let beyond = piece.t[0] < room_lo || piece.t[1] > room_hi;
            let mut rounding = 0.0_f64;
            for i in 0..4 {
                let row: Vec<Hpt> = columns
                    .iter()
                    .map(|c| c.get(i).copied().unwrap_or((Vec3::ZERO, f64::NAN)))
                    .collect();
                if beyond {
                    let (x, e) = extrapolate_curve(&row, piece.t[0], piece.t[1])?;
                    rounding = rounding.max(e);
                    cut.push(x);
                } else {
                    cut.push(sub_curve(&row, piece.t[0].max(0.0), piece.t[1].min(1.0)));
                }
            }
            let mut dist = net_distance(&piece.net, &cut)?;
            if beyond {
                // the cut rows are off the exact ones by `rounding` in each homogeneous
                // coordinate (two for a point's three): a point `N / W` moves by at most
                // `(2 e + |P| e) / W_min`, `W` convex in the weights, `P` in the points
                let flat: Vec<Hpt> = cut.iter().flatten().copied().collect();
                let w_min = flat.iter().fold(f64::INFINITY, |m, q| m.min(q.1)) - rounding;
                let reach_p = flat
                    .iter()
                    .fold(0.0_f64, |m, &(p, wq)| m.max(p.length() / wq));
                if w_min.partial_cmp(&0.0) != Some(Ordering::Greater) {
                    return Err(SolidError::invalid_input(
                        OP,
                        "a weight of the definition's rows past the section is not positive",
                    ));
                }
                dist += rounding * (2.0 + reach_p) / w_min;
                v_lo = v_lo.min(w_min);
                reach = reach.max(
                    (2.0 * piece.t[0] - 1.0)
                        .abs()
                        .max((2.0 * piece.t[1] - 1.0).abs()),
                );
                across = Some(match across {
                    Some([x, y]) => [x.min(piece.t[0]), y.max(piece.t[1])],
                    None => piece.t,
                });
            }
            let slot = if beyond {
                &mut delta_out
            } else {
                &mut delta_in
            };
            *slot = Some(slot.map_or(dist, |x| x.max(dist)));
        }
        // the bound proved: the certificate's, the net's distance from these rows, and how
        // far these rows may be from the exact ones (the floor). Over the pieces in the
        // section's range it is the certificate plus their distance; over those past it,
        // the certificate grown by the continued section's growth (Chebyshev's extremal
        // property) plus theirs.
        let delta_in = delta_in.map(|x| x + shift);
        let delta_out = delta_out.map(|x| x + shift);
        let v_hi = w.iter().fold(0.0_f64, |m, &x| m.max(x));
        // the growth is known before the certificate unless the thumbweight is the equal
        // area one, enclosed by the certificate itself (stated limit: then the certificate
        // steers by the pieces in range, and a landing the growth takes over the bound is
        // refused below, never accepted)
        let early = match (across, equal_area) {
            (Some(ext), false) => Some(section_growth(
                d.order,
                ld,
                reach,
                ext,
                [v_hi, v_lo],
                Some([d.thumbweight, d.thumbweight]),
            )?),
            _ => None,
        };
        let mut target = f64::INFINITY;
        if let Some(x) = delta_in {
            target = target.min(bound - x - floor);
        }
        if let (Some(x), Some(g)) = (delta_out, early) {
            target = target.min((bound - x - floor) / g);
        }
        if delta_out.is_some() && early.is_none() {
            target = target.min(bound - delta_out.unwrap_or(0.0) - floor);
        }
        if target.partial_cmp(&0.0) != Some(Ordering::Greater) {
            return Err(SolidError::invalid_input(
                OP,
                "the net is not within its bound of its definition",
            ));
        }
        // to the target: the bound was proved before, and the levels' halving guess must
        // not stop this run a level short of it where its rounding differs from that run's
        let mut reuse = SpanReuse {
            to_target: true,
            ..SpanReuse::default()
        };
        let certified = gn_certify_span_reusing(
            k,
            face_n,
            &leader_n,
            faces,
            &d.leader,
            a,
            b,
            r,
            d.sides,
            n,
            target,
            if equal_area { Some(&mut vary) } else { None },
            law.as_ref(),
            if any_defs { Some(&mut defs) } else { None },
            &mut reuse,
        )?;
        let late = match (across, early) {
            (Some(ext), None) => Some(section_growth(
                d.order,
                ld,
                reach,
                ext,
                [v_hi, v_lo],
                Some(vary.w),
            )?),
            _ => early,
        };
        let mut total = 0.0_f64;
        if let Some(x) = delta_in {
            total = total.max(certified + x + floor);
        }
        if let (Some(x), Some(g)) = (delta_out, late) {
            total = total.max(certified * g + x + floor);
        }
        if !matches!(
            total.partial_cmp(&bound),
            Some(Ordering::Less | Ordering::Equal)
        ) {
            return Err(SolidError::invalid_input(
                OP,
                "the net is not within its bound of its definition",
            ));
        }
        achieved = achieved.max(total);
    }
    let [lo, hi] = vary.w;
    Ok((achieved, (equal_area && lo <= hi).then_some([lo, hi])))
}

/// The growth, past the section's `[0, 1]` over `ext`, of a distance from the continued
/// section proved on `[0, 1]` ([`gn_certify_span`]): [`growth`] with `m` twice the
/// section's degree (the rows are of it), `x` the farthest `|2 t - 1|` (`reach`), the
/// section's weight over `[0, 1]` and `ext` ([`section_weight_range`]) and `V = [max,
/// min]` the rows' over `[0, 1]` and over the part past it. `thumbweight` is the G^n
/// section's proved enclosure of its thumbweight (`None` where not yet enclosed).
///
/// # Errors
///
/// `ExtendRefused` where the section's weight vanishes past `[0, 1]`; `InvalidInput` for rows
/// not of the section's degree, a thumbweight not enclosed, a weight of the rows not
/// positive or a growth not finite.
fn section_growth(
    order: u32,
    ld: usize,
    reach: f64,
    ext: [f64; 2],
    [v_hi, v_lo]: [f64; 2],
    thumbweight: Option<[f64; 2]>,
) -> Result<f64> {
    let degree = section_degree(order);
    if ld != degree + 1 {
        return Err(SolidError::invalid_input(
            OP,
            "the net's rows are not of its section's degree",
        ));
    }
    let (w_hi, w_lo) = section_weight_range(order, [0.0, 1.0], ext, || {
        thumbweight.ok_or_else(|| {
            SolidError::invalid_input(OP, "the thumbweight is not enclosed over the section")
        })
    })?;
    if !(v_lo > 0.0 && v_hi.is_finite()) {
        return Err(SolidError::invalid_input(
            OP,
            "a weight of the definition's rows past the section is not positive",
        ));
    }
    let g = growth(2 * degree, reach, (w_hi, w_lo), (v_hi, v_lo));
    if g.is_finite() {
        Ok(g)
    } else {
        Err(SolidError::invalid_input(
            OP,
            "the continued section's growth past its range is not finite",
        ))
    }
}

/// The stations of a definition at `sigma`, its seeds' knots, where `needed`: each solved
/// from the seeds on the pieces after it and (but the first) on the pieces before it, the
/// feet's sides there the seeds' own.
///
/// # Errors
///
/// Those of the seeds' evaluations and the station solves.
fn definition_stations(
    k: &Kernel,
    system: &StationSystem<'_>,
    seeds: [&BSplineCurve; 2],
    sigma: &[f64],
    needed: &[bool],
) -> Result<Vec<Option<GnStation>>> {
    let mut stations: Vec<Option<GnStation>> = vec![None; sigma.len()];
    for (j, (&s, &needed)) in sigma.iter().zip(needed).enumerate() {
        if !needed {
            continue;
        }
        k.check_cancelled(OP)?;
        let mut seed = [0.0_f64; 4];
        let mut after = [0_i32; 4];
        let mut before = [0_i32; 4];
        for (f, sd) in seeds.into_iter().enumerate() {
            let p = sd.evaluate_point(s)?;
            let right = sd.evaluate(s, 1, false, false)?;
            let left = sd.evaluate(s, 1, true, false)?;
            let (Some(dr), Some(dl)) = (right.get(1), left.get(1)) else {
                return Err(SolidError::sm_err(OP));
            };
            for (c, (x, (r1, l1))) in [(p.x, (dr.x, dl.x)), (p.y, (dr.y, dl.y))]
                .into_iter()
                .enumerate()
            {
                let q = 2 * f + c;
                if let (Some(sq), Some(aq), Some(bq)) =
                    (seed.get_mut(q), after.get_mut(q), before.get_mut(q))
                {
                    *sq = x;
                    *aq = sign(r1);
                    *bq = -sign(l1);
                }
            }
        }
        let mut st = system.solve_after(k, s, seed, Some(after))?;
        st.before = before;
        if j > 0 {
            system.solve_before(k, &mut st)?;
        }
        if let Some(slot) = stations.get_mut(j) {
            *slot = Some(st);
        }
    }
    Ok(stations)
}

/// Refuses a span between stations `a` and `b` over which a foot crosses a knot line of
/// its face (`face_n`): the construction puts a station there.
///
/// # Errors
///
/// `InvalidInput` naming the crossing.
fn refuse_knot_crossing(face_n: [&Nurbs; 2], a: &GnStation, b: &GnStation) -> Result<()> {
    for (f, nf) in face_n.iter().enumerate() {
        for c in 0..2 {
            let q = 2 * f + c;
            let (xa, xb) = (param(a, q, 0, 0), param(b, q, 0, 0));
            if inner_knots(nf, c)
                .iter()
                .any(|&dk| crosses_knot(xa, xb, dk))
            {
                return Err(SolidError::invalid_input(
                    OP,
                    "a foot crosses a face's knot line between two stations",
                ));
            }
        }
    }
    Ok(())
}

/// The net of a definition on its own stations, the seeds' knots (#2487): the
/// construction's Hermite rows there ([`NetForm::span`], joined as `Build` joins them,
/// [`net_of_spans`]) over the definition's (s, t), each span certified
/// ([`gn_certify_span`], refined toward `target`), and the net's bound as `Build` stores
/// it: the certificate's, the net's rounding off its rows and three floors. A definition
/// made from another (the offset of a round, its supports offset and its radius moved)
/// takes its net here, in its own parameters (the identity map).
///
/// # Errors
///
/// `InvalidInput` for seeds on different stations, a foot crossing a face's knot line
/// between two of them, or a span not proved; those of [`certificate_faces`] (a support the
/// certificate does not enclose), the station solves and the net.
pub(crate) fn net_of_definition(
    k: &Kernel,
    pb: &ProceduralBlend,
    target: f64,
) -> Result<(BSplineSurface, f64)> {
    let (ss, spans, certified, ld, _) = spans_of_definition(k, pb, target, None)?;
    let net = net_of_spans(&ss, &spans, ld)?;
    let faces = pb.supports();
    let bound = certified
        + stored_net_distance(&net, &spans, ld)?
        + 3.0 * precision_floor(k, faces, pb.definition().order, &net)?;
    Ok((net, bound))
}

/// One span of a definition's net, as [`net_of_definition`] makes it: its homogeneous rows
/// `W P` and weights `W` (4 along s by `ld` across, `[i * ld + j]`, from the lower station
/// to the higher), its certificate and `ld`.
pub(crate) struct DefinitionSpan {
    pub(crate) homog: Vec<Vec3>,
    pub(crate) weights: Vec<f64>,
    pub(crate) certified: f64,
    pub(crate) ld: usize,
    /// The span's two stations, the lower s first.
    pub(crate) stations: Vec<GnStation>,
}

/// The span of a definition's net from its station `first` to the next, solved and
/// certified as [`net_of_definition`] does each.
///
/// # Errors
///
/// Those of [`net_of_definition`].
pub(crate) fn span_of_definition(
    k: &Kernel,
    pb: &ProceduralBlend,
    first: usize,
    target: f64,
) -> Result<DefinitionSpan> {
    let (_, mut spans, certified, ld, stations) = spans_of_definition(k, pb, target, Some(first))?;
    let (homog, weights) = spans.pop().ok_or_else(|| SolidError::sm_err(OP))?;
    Ok(DefinitionSpan {
        homog,
        weights,
        certified,
        ld,
        stations,
    })
}

/// The stations' s, the spans, the largest certificate, the width across and the stations.
type SpansOfDefinition = (
    Vec<f64>,
    Vec<(Vec<Vec3>, Vec<f64>)>,
    f64,
    usize,
    Vec<GnStation>,
);

/// The spans of a definition's net on its stations (all, or only the one from station
/// `only`), each certified ([`SpansOfDefinition`]).
fn spans_of_definition(
    k: &Kernel,
    pb: &ProceduralBlend,
    target: f64,
    only: Option<usize>,
) -> Result<SpansOfDefinition> {
    let d = pb.definition();
    let n = d.order as usize;
    let equal_area = n >= 2 && d.thumbweight == GN_THUMBWEIGHT_EQUAL_AREA;
    let net_form = NetForm {
        n,
        thumbweight: d.thumbweight,
        equal_area,
        law: d.law.is_some(),
    };
    let ld = net_form.width();
    let faces = pb.supports();
    let ([n0, n1], mut defs) = certificate_faces(k, faces, n, d.law.is_some())?;
    let any_defs = defs.faces.iter().any(Option::is_some);
    let leader_n = NurbsCurve::of(&d.leader)?;
    let law = match &d.law {
        Some(c) => Some(GnLaw::of(c)?),
        None => None,
    };
    let [seed0, seed1] = &d.seeds;
    let sigma = seed0.knots(None)?.0;
    if seed1.knots(None)?.0 != sigma || sigma.len() < 2 {
        return Err(SolidError::invalid_input(
            OP,
            "the seeds are not on one set of stations",
        ));
    }
    let r = match &law {
        Some(l) => {
            let first = sigma.first().copied().unwrap_or(f64::NAN);
            let r0 = l.curve.evaluate_point(first)?.x;
            [r0, r0]
        }
        None => d.offsets,
    };
    let tr = Trunc::new(
        if law.is_some() { 2 } else { 1 },
        if n >= 2 { n } else { 0 },
    )?;
    let system = StationSystem {
        faces,
        leader: &d.leader,
        r,
        side: d.sides,
        tr,
        n,
        equal_area,
        law: law.as_ref(),
    };
    let needed: Vec<bool> = (0..sigma.len())
        .map(|j| only.is_none_or(|i| j == i || Some(j) == i.checked_add(1)))
        .collect();
    let stations = definition_stations(k, &system, [seed0, seed1], &sigma, &needed)?;
    let solved: Vec<GnStation> = stations.into_iter().flatten().collect();
    if solved.len() != needed.iter().filter(|&&x| x).count() || solved.len() < 2 {
        return Err(SolidError::sm_err(OP));
    }
    let mut vary = varying(n, r, &solved, equal_area);
    let mut spans = Vec::new();
    let mut certified = 0.0_f64;
    for pair in solved.windows(2) {
        k.check_cancelled(OP)?;
        let [a, b] = pair else {
            return Err(SolidError::sm_err(OP));
        };
        refuse_knot_crossing([&n0, &n1], a, b)?;
        let (ra, rb) = (net_form.station_rows(a)?, net_form.station_rows(b)?);
        spans.push(net_form.span(a, &ra, b, &rb));
        let c = gn_certify_span(
            k,
            [&n0, &n1],
            &leader_n,
            faces,
            &d.leader,
            a,
            b,
            r,
            d.sides,
            n,
            target,
            if equal_area { Some(&mut vary) } else { None },
            law.as_ref(),
            if any_defs { Some(&mut defs) } else { None },
        )?;
        if !c.is_finite() {
            return Err(SolidError::invalid_input(
                OP,
                "a span of the definition's net is not proved",
            ));
        }
        certified = certified.max(c);
    }
    let ss: Vec<f64> = solved.iter().map(|st| st.s).collect();
    Ok((ss, spans, certified, ld, solved))
}

/// -1, 0 or +1.
fn sign(x: f64) -> i32 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `T_m(x)` for `x >= 1` by its closed form, independent of the recurrence.
    fn cheb(m: usize, x: f64) -> f64 {
        (m as f64 * x.acosh()).cosh()
    }

    /// The circle section's weight `1 - 2 t (1 - t) (1 - c)` is at most 1 over the
    /// certificate's `[0, 1]` (the `max W` of the growth: the distance is proved there) and
    /// exceeds 1 off it; the growth is the closed form of
    /// definitions-and-certificates 6.4 with that `W`, the chord's being 1.
    #[test]
    fn the_growth_past_the_section_is_the_closed_form() {
        let ext = [-0.697_676_318_880_472_2, 0.0];
        let (reach, v) = (2.0 * 0.697_676_318_880_472_2 + 1.0, [1.5, 0.75]);
        let ld = 3;
        let m = 2 * (ld - 1);
        let want = cheb(m, reach) * (1.0 * v[0]) / (1.0 * v[1]) * (1.0 + gamma(4 * m + 16));
        let got = section_growth(1, ld, reach, ext, v, None).unwrap();
        assert!(
            (got - want).abs() <= 1e-12 * want,
            "circle growth {got}, wanted {want}"
        );
        let chord = section_growth(0, 2, reach, ext, v, None).unwrap();
        let want = cheb(2, reach) * v[0] / v[1] * (1.0 + gamma(4 * 2 + 16));
        assert!((chord - want).abs() <= 1e-12 * want, "chord growth {chord}");
    }

    /// The circle's weight over a certified range outside `[0, 1]` (an extension of an
    /// extension) reaches `1 + 2 max(0, t (t - 1))`, the chord's stays 1.
    #[test]
    fn the_circles_weight_past_its_arc_is_above_one() {
        let t = 0.697_676_318_880_472_2;
        let (hi, lo) =
            section_weight_range(1, [-t, 0.0], [-2.0 * t, -t], || Ok([0.0, 0.0])).unwrap();
        assert!(
            (hi - (1.0 + 2.0 * t * (1.0 + t))).abs() <= 1e-15 * hi,
            "hi {hi}"
        );
        assert_eq!(lo, 1.0);
        let chord = section_weight_range(0, [-t, 0.0], [-2.0 * t, -t], || Ok([0.0, 0.0]));
        assert_eq!(chord.unwrap(), (1.0, 1.0));
    }

    /// A rational curve of degree 7 (weights positive), as homogeneous control points.
    fn curve() -> Vec<Hpt> {
        (0..8)
            .map(|i| {
                let x = f64::from(i);
                (
                    Vec3::new(x, (0.7 * x).sin() * 3.0, (0.3 * x * x).cos()),
                    1.0 + 0.3 * (1.3 * x).sin(),
                )
            })
            .collect()
    }

    /// The homogeneous value at `tau` by the Bernstein form (binomials and powers, no
    /// de Casteljau: independent of the code under test).
    fn at(pts: &[Hpt], tau: f64) -> (Vec3, f64) {
        let d = pts.len() - 1;
        let mut binom = 1.0_f64;
        let (mut p, mut w) = (Vec3::ZERO, 0.0);
        for (i, &(q, wq)) in pts.iter().enumerate() {
            let b = binom * tau.powi(i as i32) * (1.0 - tau).powi((d - i) as i32);
            p += q * b;
            w += wq * b;
            binom = binom * (d - i) as f64 / (i + 1) as f64;
        }
        (p, w)
    }

    fn off(a: (Vec3, f64), b: (Vec3, f64)) -> f64 {
        let d = a.0 - b.0;
        d.x.abs()
            .max(d.y.abs())
            .max(d.z.abs())
            .max((a.1 - b.1).abs())
    }

    #[test]
    fn extrapolating_over_0_1_returns_the_curve_and_agrees_with_sub_curve_inside() {
        let c = curve();
        let (same, _) = extrapolate_curve(&c, 0.0, 1.0).unwrap();
        assert_eq!(same, c);
        let (cut, _) = extrapolate_curve(&c, 0.2, 0.7).unwrap();
        let want = sub_curve(&c, 0.2, 0.7);
        for (x, y) in cut.iter().zip(&want) {
            assert!(off(*x, *y) < 1e-13, "{x:?} against {y:?}");
        }
    }

    #[test]
    fn extrapolating_past_0_1_is_the_same_curve_within_its_rounding() {
        let c = curve();
        for (lo, hi) in [(-0.7, 0.0), (-1.4, 0.1), (0.9, 1.6), (-0.3, 1.2)] {
            let (cut, e) = extrapolate_curve(&c, lo, hi).unwrap();
            let mut worst = 0.0_f64;
            for j in 0..=10 {
                let u = f64::from(j) / 10.0;
                worst = worst.max(off(at(&cut, u), at(&c, lo + u * (hi - lo))));
            }
            assert!(worst > 0.0 || e > 0.0);
            assert!(
                worst <= e,
                "[{lo}, {hi}]: observed {worst} over its bound {e}"
            );
            assert!(worst < 1e-9, "[{lo}, {hi}]: {worst} is not the same curve");
        }
    }

    use crate::math::Point2;
    use crate::nurbs::b_spline_surface::SurfParamType;

    fn wall(dz: f64) -> BSplineSurface {
        BSplineSurface::create_bilinear_surface(
            Vec3::new(0.0, 0.0, dz),
            Vec3::new(2.0, 0.0, dz),
            Vec3::new(0.0, 1.0, dz),
            Vec3::new(2.0, 1.0, 0.5 + dz),
        )
        .unwrap()
    }

    #[test]
    fn mapped_nets_certify_identity_swaps_reversals_and_restrictions() {
        let expected = wall(0.0);
        let check = |actual: &BSplineSurface, a, b, swapped| {
            let bound = mapped_net_distance(actual, &expected, a, b, swapped).unwrap();
            assert!(bound < 1e-10, "{bound}");
        };
        check(&expected, [0.0; 2], [1.0; 2], false);
        let mut swapped = expected.copy();
        swapped.swap_uv().unwrap();
        check(&swapped, [0.0; 2], [1.0; 2], true);
        let reversed = BSplineSurface::create_bilinear_surface(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 1.0, 0.5),
            Vec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
        check(&reversed, [1.0, 0.0], [-1.0, 1.0], false);
        let restricted = expected
            .split_at(0.25, SurfParamType::U)
            .unwrap()
            .1
            .split_at(0.75, SurfParamType::U)
            .unwrap()
            .0
            .split_at(0.5, SurfParamType::V)
            .unwrap()
            .1;
        check(&restricted, [0.0; 2], [1.0; 2], false);
        let nurb = expected.gw_nurb().unwrap();
        let rows = nurb.rows().map(<[_]>::to_vec).collect::<Vec<_>>();
        let reparameterized = BSplineSurface::from_data(
            1,
            1,
            1,
            1,
            3,
            3,
            &rows,
            &[-2.0, -2.0, 2.0, 2.0],
            &[4.0, 4.0, 8.0, 8.0],
        )
        .unwrap();
        check(&reparameterized, [0.5, -1.0], [0.25, 0.25], false);
    }

    #[test]
    fn mapped_net_bound_encloses_a_real_displacement() {
        let (actual, expected) = (wall(0.125), wall(0.0));
        let bound = mapped_net_distance(&actual, &expected, [0.0; 2], [1.0; 2], false).unwrap();
        assert!((0.125..0.125 + 1e-10).contains(&bound), "{bound}");
        for u in [0.0, 0.25, 0.75, 1.0] {
            for v in [0.0, 0.5, 1.0] {
                let uv = Point2::new(u, v);
                let difference = (actual.evaluate_point(uv).unwrap()
                    - expected.evaluate_point(uv).unwrap())
                .length();
                assert!(difference <= bound);
            }
        }
    }

    #[test]
    fn clamped_mapped_certificate_covers_boundary_strips_and_corners() {
        let expected = wall(0.0);
        let nurb = expected.gw_nurb().unwrap();
        let rows = nurb.rows().map(<[_]>::to_vec).collect::<Vec<_>>();
        let actual = BSplineSurface::from_data(
            1,
            1,
            1,
            1,
            3,
            3,
            &rows,
            &[-1.0, -1.0, 2.0, 2.0],
            &[3.0, 3.0, 9.0, 9.0],
        )
        .unwrap();
        let (a, b) = ([1.0 / 3.0, -0.5], [1.0 / 3.0, 1.0 / 6.0]);
        assert!(mapped_net_distance(&actual, &expected, a, b, false).is_err());
        let bound = mapped_clamped_net_distance(&actual, &expected, a, b, false).unwrap();
        assert!(bound < 1e-10, "{bound}");
        // The complete mapped domain lies beyond BOTH lower boundaries: the expected
        // surface is its constant corner. Omitting the strips would falsely certify zero.
        let bound =
            mapped_clamped_net_distance(&expected, &expected, [-1.0; 2], [0.25; 2], false).unwrap();
        let distance = Vec3::new(2.0, 1.0, 0.5).length();
        assert!(bound >= distance && bound < distance + 1e-10, "{bound}");
    }

    #[test]
    fn mapped_net_certificate_refines_a_smooth_rational_profile() {
        use crate::nurbs::math::CPoint;
        let rows = [
            [(0.0, 0.0, 0.0, 1.0), (0.0, 1.0, 0.0, 1.0)],
            [(0.25, 0.0, 0.125, 0.75), (0.25, 0.75, 0.25, 0.75)],
            [(1.0, 0.0, 0.25, 1.25), (1.0, 1.25, 0.5, 1.25)],
            [(2.0, 0.0, 0.0, 1.0), (2.0, 1.0, 0.5, 1.0)],
        ]
        .map(|row| row.map(|(x, y, z, w)| CPoint { x, y, z, w }).to_vec());
        let expected = BSplineSurface::from_data(
            3,
            1,
            2,
            1,
            6,
            3,
            &rows,
            &[0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
            &[0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let restricted = expected.split_at(0.25, SurfParamType::U).unwrap().1;
        for actual in [&expected, &restricted] {
            let bound = mapped_net_distance(actual, &expected, [0.0; 2], [1.0; 2], false).unwrap();
            assert!(bound < 1e-10, "{bound}");
        }
        // An underflowing product residual must never be mistaken for an exact endpoint.
        let endpoint = affine_endpoint(0.0, 1.1, f64::MIN_POSITIVE);
        assert!(endpoint.lo < endpoint.hi);
    }

    #[test]
    fn mapped_net_refuses_hostile_knots_maps_and_domains() {
        for knots in [
            vec![0.0, 0.0, f64::NAN, 1.0],
            vec![0.0, 0.0, 1.0, 0.5],
            vec![0.0, 0.0, 0.0, 0.0],
        ] {
            assert!(bezier_spans(&knots, 1, 2).is_err());
        }
        // A quadratic's simple interior knot would require an uncertified knot insertion.
        assert!(bezier_spans(&[0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0], 2, 4).is_err());
        let net = wall(0.0);
        assert!(mapped_net_distance(&net, &net, [0.0; 2], [0.0, 1.0], false).is_err());
        assert!(mapped_net_distance(&net, &net, [f64::NAN, 0.0], [1.0; 2], false).is_err());
        assert!(mapped_net_distance(&net, &net, [0.25, 0.0], [1.0; 2], false).is_err());
    }

    #[test]
    fn shared_float_net_metric_keeps_its_existing_value() {
        let actual = vec![vec![
            (Vec3::new(0.25, 0.0, 0.0), 1.0),
            (Vec3::new(2.5, 0.0, 0.0), 2.0),
        ]];
        let expected = vec![vec![(Vec3::ZERO, 1.0), (Vec3::new(1.0, 0.0, 0.0), 1.0)]];
        assert_eq!(
            net_distance(&actual, &expected).unwrap().to_bits(),
            1.25_f64.to_bits()
        );
        assert_eq!(
            sub_curve(&actual[0], 0.25, 0.75),
            vec![
                (Vec3::new(0.8125, 0.0, 0.0), 1.25),
                (Vec3::new(1.9375, 0.0, 0.0), 1.75)
            ]
        );
    }
}
