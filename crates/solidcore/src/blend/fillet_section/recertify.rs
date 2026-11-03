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
//! The bound is their sum with the floor ([`precision_floor`]), a forward error bound of
//! computing H, at most ε, or the net is refused. A blend stores ε as its certificate, the
//! net's rounding off its rows and three floors: H computed again is within two floors of
//! the H the net was made of. A net cut or reparametrized
//! after it was built (`SplitAt`, `Reparameterize`, `Reverse`, `SwapUV`) is a piece of the
//! same Hermite net: H is cut to the piece, in homogeneous coordinates, before the nets
//! are compared.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use core::cmp::Ordering;

use super::certificate::{gn_certify_span, GnLaw, NurbsCurve};
use super::section::{
    certificate_faces, crosses_knot, inner_knots, param, varying, NetForm, StationSystem,
    GN_THUMBWEIGHT_EQUAL_AREA,
};
use super::series::Trunc;
use super::station::{as_b_spline, GnStation, Nurbs, STATION_STEP};
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::NOW;
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
    let split = |p: &[Hpt], tau: f64| -> (Vec<Hpt>, Vec<Hpt>) {
        let mut level: Vec<Hpt> = p.to_vec();
        let mut left = Vec::new();
        let mut right = Vec::new();
        while let (Some(&first), Some(&last)) = (level.first(), level.last()) {
            left.push(first);
            right.push(last);
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
        right.reverse();
        (left, right)
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

/// `|B - H|` over a piece, from the two patches' control nets (module docs).
///
/// # Errors
///
/// `InvalidInput` where the nets differ in size or a weight is not positive.
pub(crate) fn net_distance(b: &[Vec<Hpt>], h: &[Vec<Hpt>]) -> Result<f64> {
    let flat = |n: &[Vec<Hpt>]| -> Vec<Hpt> { n.iter().flatten().copied().collect() };
    let (pb, ph) = (flat(b), flat(h));
    if pb.len() != ph.len() || pb.is_empty() {
        return Err(SolidError::invalid_input(
            OP,
            "the net is not of its definition's degrees",
        ));
    }
    let positive = |p: &[Hpt]| p.iter().all(|&(_, w)| w > 0.0 && w.is_finite());
    if !positive(&pb) || !positive(&ph) {
        return Err(SolidError::invalid_input(
            OP,
            "a weight of the net is not positive",
        ));
    }
    let euclid = |(p, w): Hpt| p * (1.0 / w);
    let c = ph.first().map_or(Vec3::ZERO, |&q| euclid(q));
    let mut d_points = 0.0_f64;
    let mut reach = 0.0_f64;
    for (&x, &y) in pb.iter().zip(&ph) {
        d_points = d_points.max((euclid(x) - euclid(y)).length());
        reach = reach.max((euclid(y) - c).length());
    }
    let mut cross = 0.0_f64;
    for (i, &(_, wi)) in pb.iter().enumerate() {
        let vi = ph.get(i).map_or(f64::NAN, |q| q.1);
        for (j, &(_, wj)) in pb.iter().enumerate() {
            let vj = ph.get(j).map_or(f64::NAN, |q| q.1);
            cross = cross.max((wi * vj - vi * wj).abs());
        }
    }
    let w_min = pb.iter().map(|q| q.1).fold(f64::INFINITY, f64::min);
    let v_min = ph.iter().map(|q| q.1).fold(f64::INFINITY, f64::min);
    let d = d_points + reach * cross / (w_min * v_min);
    if d.is_finite() {
        Ok(d)
    } else {
        Err(SolidError::invalid_input(OP, "the net is not finite"))
    }
}

/// `u`, the unit roundoff of `f64`.
const UNIT_ROUNDOFF: f64 = f64::EPSILON / 2.0;

/// `gamma_k = k u / (1 - k u)`: the relative error bound of k floating operations in a
/// chain (Higham, the standard model).
fn gamma(k: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let ku = k as f64 * UNIT_ROUNDOFF;
    ku / (1.0 - ku)
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
/// definition's stations (its degrees, a piece across a station or past the seeds, the
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
    let centre = frame_centre(pb.net())?;
    let local = pb.translated(k, centre * -1.0)?;
    recertify_in_frame(k, &local, pb.bound(), frame_rounding(pb.net())?)
}

/// [`recertify`] of a blend already in its own frame, against `bound`, its net's
/// distance from its rows taken as at least `shift` more.
#[allow(clippy::too_many_lines)]
fn recertify_in_frame(k: &Kernel, pb: &ProceduralBlend, bound: f64, shift: f64) -> Result<f64> {
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
        let room_t = |x: f64| map_rounding(d.map_a[1], d.map_b[1], (x - d.map_a[1]) / d.map_b[1]);
        let inside = piece.s[0] >= a - room_s(a)
            && piece.s[1] <= b + room_s(b)
            && piece.t[0] >= -room_t(0.0)
            && piece.t[1] <= 1.0 + room_t(1.0);
        if !inside {
            return Err(SolidError::invalid_input(
                OP,
                "a piece of the net runs across a station of its definition",
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
    let mut stations: Vec<Option<GnStation>> = vec![None; sigma.len()];
    for (j, (&s, &needed)) in sigma.iter().zip(&used).enumerate() {
        if !needed {
            continue;
        }
        k.check_cancelled(OP)?;
        let mut seed = [0.0_f64; 4];
        let mut after = [0_i32; 4];
        let mut before = [0_i32; 4];
        for (f, sd) in [seed0, seed1].into_iter().enumerate() {
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
    let solved: Vec<GnStation> = stations.iter().flatten().copied().collect();
    let mut vary = varying(n, r[0] * r[1], &solved, equal_area);

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
        let (ra, rb) = (net_form.station_rows(a)?, net_form.station_rows(b)?);
        let (homog, w) = net_form.span(a, &ra, b, &rb);
        let h = b.s - a.s;
        let mut delta = 0.0_f64;
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
            for i in 0..4 {
                let row: Vec<Hpt> = columns
                    .iter()
                    .map(|c| c.get(i).copied().unwrap_or((Vec3::ZERO, f64::NAN)))
                    .collect();
                cut.push(sub_curve(&row, piece.t[0].max(0.0), piece.t[1].min(1.0)));
            }
            delta = delta.max(net_distance(&piece.net, &cut)?);
        }
        // the bound proved: the certificate's, the net's distance from these rows, and how
        // far these rows may be from the exact ones (the floor)
        let delta = delta + shift;
        let target = bound - delta - floor;
        if target.partial_cmp(&0.0) != Some(Ordering::Greater) {
            return Err(SolidError::invalid_input(
                OP,
                "the net is not within its bound of its definition",
            ));
        }
        let certified = gn_certify_span(
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
        )?;
        let total = certified + delta + floor;
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
    Ok(achieved)
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
