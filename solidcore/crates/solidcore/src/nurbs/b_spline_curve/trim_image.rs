//! A certified lower bound on the radius of curvature of a UV trim's image on its surface
//! (not upstream): what lets `BSplineCurve::assert_valid`'s test 13 pass over a turn of a
//! trim that is sharp in (u, v) but smooth on the surface, as the exact trim of a smooth
//! curve passing near a sphere's pole is.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`radius_clears_on_surface`] | none |
//!
//! **The bound.** Take a piece `[a, b]` of the trim inside one of its knot spans, with
//! `t = m + h s`, `s` in `[-1, 1]`, `m` the piece's middle and `h` half its length rounded
//! up. The trim is a polynomial there: its power coefficients in `s` come from its exact
//! derivatives at `m` (in rounded intervals, [`curve_derivs_over`]). On a polynomial piece
//! (spans `ku`, `kv`) of the surface, the homogeneous surface `(H, W)` is a polynomial of
//! degree `(p, q)`, so its Taylor expansion at a point `(u0, v0)` is exact; composed with the
//! trim it gives the homogeneous image `(Q(s), W(s))` as polynomials in `s`, every
//! coefficient an interval holding the exact one. `Q` is taken about a point `s0` near the
//! image (`H - s0 W`), which moves the image and changes none of its derivatives.
//!
//! The image is `g = Q / W + s0`, `g' = N / W^2` with `N = Q' W - Q W'`, and
//! `g'' = (N' W - 2 N W') / W^3`, so `g' x g'' = (N x N') / W^4`. Over `s` in `[-1, 1]` a polynomial's value lies within the
//! sum of its coefficients' magnitudes, and its value along a direction `n` is at least its
//! constant term less the other terms' magnitudes. So
//! - `W >= W_lo = w_0 - sum |w_j|` (it must be positive) and `W <= W_hi`;
//! - `|g'| >= (N . n)_lo / (|n| W_hi^2)`, with `n` the middle of `N`'s constant term;
//! - `|g' x g''| = |N x N'| / W^4 <= |N x N'|_hi / W_lo^4`, with `N x N'` composed
//!   coefficient by coefficient, so the parameter's own acceleration along the curve
//!   cancels there and does not enter the bound;
//! - the radius `|g'|^3 / |g' x g''|` (the same in every parameter) is at least the
//!   quotient of those bounds.
//!
//! The piece clears the tolerance where that lower bound is at least it. The bounds are
//! taken from the composed coefficients, never from enclosures of the surface's partials
//! over a box: near a pole the partials are large and cancel to a small image speed, and an
//! interval enclosure over a box loses that cancellation (its width grows with the box,
//! not with the speed), where the composed coefficients keep it to rounding.
//!
//! **Pieces.** `[lo, hi]` is cut at the trim's knots. A piece whose points can lie on more
//! than [`MAX_PATCHES`] polynomial pieces of the surface, whose rounded (u, v) range runs off
//! the surface's knots, or that does not clear, is halved,
//! breadth first. Every point of a piece lies on one of the surface pieces its parameter
//! range meets, and each of those pieces' polynomials is the surface at the points on it, so
//! the piece clears only where it clears on every one of them.
//!
//! **Rounding.** Every coefficient is computed in rounded interval arithmetic
//! ([`Ivl`]): the surface's homogeneous control points and a non-rational trim's control
//! points enter exactly (`Nurbs::of` and `NurbsCurve::of` copy them; a weight 1 multiplies
//! exactly), the basis functions' derivatives are rounded intervals ([`ders_basis`]), and so
//! is every sum and product after them. A NaN or an infinite bound clears nothing.
//!
//! Deviations: not upstream. No recursion: a queue of pieces, at most
//! [`MAX_PIECES`] enclosed and each halved at most [`MAX_HALVINGS`] times; cancellation is
//! polled once per piece. A rational trim and a trim on a procedural blend (whose NURBS
//! net only approximates its positions) are not certified (the caller reports as before).
//! A trim the bound fails to clear costs every piece the budget allows (up to
//! [`MAX_PIECES`] enclosures) before the solution is reported.

use std::collections::VecDeque;

use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{as_b_spline, ders_basis, span, Nurbs};
use crate::error::Result;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::surface::surface::{Surface, SurfaceKind};

/// The operation path for cancellation.
const OP: &str = "nurbs::b_spline_curve::trim_image::radius_clears_on_surface";

/// How many pieces [`radius_clears_on_surface`] encloses at most before it gives up.
pub(crate) const MAX_PIECES: usize = 4096;

/// How many times one piece is halved at most before [`radius_clears_on_surface`] gives up.
pub(crate) const MAX_HALVINGS: u32 = 64;

/// How many polynomial pieces of the surface one piece of the trim may meet before it is
/// halved instead of enclosed on each.
const MAX_PATCHES: usize = 4;

/// How far, relative to the knots' largest end (or 1), a trim piece's parameter range may
/// pass the surface's knot range and still be enclosed on the end span: 64 units in the
/// last place, far above the rounding a point on the domain's boundary is widened by, and
/// far below a trim that truly leaves the domain. In 3D the stretch enclosed on the end
/// span's polynomial past the knots is at most `|∇S|` times that UV slack; that it holds
/// no feature of the surface is ARGUED (it is rounding of a point on the boundary), not
/// bounded.
const KNOT_RANGE_SLACK: f64 = 64.0 * f64::EPSILON;

/// A polynomial in `s` by its power coefficients, each an interval.
type Poly = Vec<Ivl>;

/// Whether the image of the UV trim `trim` on `surface`, the 3D curve `S(c(t))`, has a
/// radius of curvature of at least `tol` at every parameter of `[lo, hi]` (from either side
/// of a knot of the trim or of the surface): certified as the module docs describe.
///
/// `false` (not certified: the caller reports as before) for a rational trim, a trim or
/// surface without a NURBS form, a procedural blend (its net only approximates it), a
/// piece that does not clear or whose range runs off the surface's knots after
/// [`MAX_HALVINGS`] halvings, more than [`MAX_PIECES`] pieces, or a bound that is not
/// finite.
///
/// # Errors
///
/// [`crate::error::SolidError::Unported`] and cancellation.
pub(crate) fn radius_clears_on_surface(
    k: &Kernel,
    trim: &BSplineCurve,
    surface: &Surface,
    lo: f64,
    hi: f64,
    tol: f64,
) -> Result<bool> {
    // (written to accept: a NaN fails)
    let ordered = lo <= hi;
    if !ordered || !tol.is_finite() {
        return Ok(false);
    }
    let curve = match NurbsCurve::of(trim) {
        Ok(c) => c,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(false),
    };
    let polynomial = curve.pw.chunks(4).all(|c| c.get(3) == Some(&1.0));
    if !polynomial {
        return Ok(false);
    }
    // a knot of full multiplicity inside the trim can carry a tangent jump (a C0 corner)
    // that the pieces, cut at the knots, never see: not certified unless its two tangents
    // are certified parallel
    if has_corner_knot(&curve, lo, hi) {
        return Ok(false);
    }
    // a procedural blend's net only approximates its positions: a bound on the net is no
    // bound on the surface, so its trims are not certified
    match surface.kind_with_nurb(k) {
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Ok(SurfaceKind::ProceduralBlend(_)) | Err(_) => return Ok(false),
        Ok(_) => {}
    }
    let net = match as_b_spline(k, surface) {
        Ok(Some(bs)) => match Nurbs::of(&bs) {
            Ok(n) => n,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(false),
        },
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Ok(None) | Err(_) => return Ok(false),
    };
    let mut cuts = vec![lo];
    for &x in &curve.u {
        if x > lo && x < hi && cuts.last().is_some_and(|&l| x > l) {
            cuts.push(x);
        }
    }
    cuts.push(hi);
    let mut pieces: VecDeque<(f64, f64, u32)> = cuts
        .iter()
        .zip(cuts.iter().skip(1))
        .map(|(&a, &b)| (a, b, 0))
        .collect();
    let mut enclosed = 0usize;
    while let Some((a, b, halvings)) = pieces.pop_front() {
        k.check_cancelled(OP)?;
        enclosed += 1;
        if enclosed > MAX_PIECES {
            return Ok(false);
        }
        // a piece whose rounded (u, v) range runs off the surface's knots (easy near a pole
        // on the domain's boundary) is halved like one that does not clear
        if piece_clears(&curve, &net, a, b, tol) == Some(true) {
            continue;
        }
        let m = 0.5 * (a + b);
        let splits = a < m && m < b;
        if halvings >= MAX_HALVINGS || !splits {
            return Ok(false);
        }
        pieces.push_back((a, m, halvings + 1));
        pieces.push_back((m, b, halvings + 1));
    }
    Ok(true)
}

/// Whether `curve` has a knot strictly inside its domain and within `[lo, hi]` (ends
/// included) where it may turn a corner: a knot of multiplicity above its degree (the
/// curve may jump there), or of multiplicity equal to its degree (C0) whose left and right
/// tangents are not parallel to rounding. The tangents on both sides are enclosed in
/// rounded intervals ([`curve_derivs_over`] on each side's span). They are parallel to
/// rounding where the cross product's enclosure is at most [`PARALLEL_ULPS`] `ε` times
/// the product of their lengths plus twice each enclosure's half-width times the other's
/// length (the rounding of the derivatives' sums, which cancel by the trim's coordinates
/// over its knot spans), and the dot product's is positive: a G1 join is not a corner.
/// That a turn below that is rounding, not a corner, is ARGUED. A side
/// whose tangent can vanish is a corner.
fn has_corner_knot(curve: &NurbsCurve, lo: f64, hi: f64) -> bool {
    let (Some(&first), Some(&last)) = (curve.u.get(curve.p), curve.u.get(curve.n)) else {
        return true;
    };
    let mut i = 0;
    while let Some(&x) = curve.u.get(i) {
        let mult = curve.u.iter().skip(i).take_while(|&&y| y == x).count();
        let inside = x > first && x < last && x >= lo && x <= hi;
        if inside && mult > curve.p {
            return true;
        }
        if inside && mult == curve.p {
            let (Some(left), right) = (i.checked_sub(1), i + mult - 1) else {
                return true;
            };
            let tangent = |piece: usize| {
                curve_derivs_over(curve, Ivl::from(x), 1, Some(piece))
                    .ok()
                    .and_then(|d| d.get(1).copied())
            };
            let (Some(a), Some(b)) = (tangent(left), tangent(right)) else {
                return true;
            };
            let cross = a.x * b.y - a.y * b.x;
            let dot = a.x * b.x + a.y * b.y;
            let len = |v: V3<Ivl>| (v.x * v.x + v.y * v.y).sqrt();
            // the enclosures' own widths: the rounding of the derivatives' sums
            let width = |v: V3<Ivl>| {
                let half = |x: Ivl| Ivl::from(0.5) * (Ivl::from(x.hi) - Ivl::from(x.lo));
                (half(v.x) * half(v.x) + half(v.y) * half(v.y)).sqrt()
            };
            let (la, lb) = (len(a), len(b));
            let rounding = (Ivl::from(PARALLEL_ULPS * f64::EPSILON) * la * lb
                + Ivl::from(2.0) * (width(a) * lb + la * width(b)))
            .hi;
            let parallel = cross.mag() <= rounding && dot.lo > 0.0;
            if !parallel {
                return true;
            }
        }
        i += mult.max(1);
    }
    false
}

/// The turn, in units of `ε` (as a cross product over the lengths), below which
/// [`has_corner_knot`] reads a C0 knot's two tangents as parallel to rounding.
const PARALLEL_ULPS: f64 = 64.0;

/// Not upstream: whether the piece `[lo, hi]` of the trim `trim`, inside one of its knot
/// spans, is a sliver at rounding level, in UV and on `surface`:
/// - its UV diameter is at most `length`: a bound from the piece's power coefficients
///   about its middle, enclosed in rounded intervals as [`piece_clears`] encloses them
///   (every point is the middle's point plus the higher terms, each at most its
///   coefficient's magnitude over `s` in `[-1, 1]`);
/// - its image's 3D diameter, bounded by the UV diameter times an enclosure of the
///   surface's Jacobian norm `sqrt(|S_u|² + |S_v|²)` over the piece's UV box (from the
///   net's basis functions and their first derivatives over the box and the quotient rule,
///   in rounded intervals, on every polynomial patch the box meets), is at most
///   [`SLIVER_3D_ULPS`] `ε` times the larger of the surface net's largest coordinate and
///   the Jacobian bound times the trim's largest coordinate (the model's rounding, or the
///   trim's rounding carried to 3D). Given the UV bound this holds wherever the Jacobian
///   is enclosed; what it adds is that enclosure, so the sliver's 3D size is certified.
///
/// `false` for a rational trim, a piece that crosses a knot or is not ordered, a procedural
/// blend, and a bound that is not finite.
///
/// # Errors
///
/// A [`crate::error::SolidError::Unported`] or `Cancelled` of the trim's or the surface's
/// data.
pub(crate) fn sliver_within(
    k: &Kernel,
    trim: &BSplineCurve,
    surface: &Surface,
    lo: f64,
    hi: f64,
    length: f64,
) -> Result<bool> {
    // (written to accept: a NaN fails)
    let ordered = lo <= hi;
    if !ordered || !length.is_finite() {
        return Ok(false);
    }
    let curve = match NurbsCurve::of(trim) {
        Ok(c) => c,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(false),
    };
    let polynomial = curve.pw.chunks(4).all(|c| c.get(3) == Some(&1.0));
    if !polynomial {
        return Ok(false);
    }
    let m = 0.5 * (lo + hi);
    let gap = if hi - m > m - lo { hi - m } else { m - lo };
    let h = Ivl::from(round_up(gap));
    let span_t = span(&curve.u, curve.p, curve.n, m);
    let (Some(&k0), Some(&k1)) = (curve.u.get(span_t), curve.u.get(span_t + 1)) else {
        return Ok(false);
    };
    let inside = k0 <= lo && hi <= k1;
    if !inside {
        return Ok(false);
    }
    let Ok(d) = curve_derivs_over(&curve, Ivl::from(m), curve.p, Some(span_t)) else {
        return Ok(false);
    };
    let (Some(d0), mut ru, mut rv) = (d.first().copied(), Ivl::from(0.0), Ivl::from(0.0)) else {
        return Ok(false);
    };
    let mut scale = Ivl::from(1.0);
    for (j, dj) in d.iter().enumerate().skip(1) {
        scale = scale * h / Ivl::from(j as f64);
        ru = ru + Ivl::from((dj.x * scale).mag());
        rv = rv + Ivl::from((dj.y * scale).mag());
    }
    let diameter = Ivl::from(2.0) * (ru * ru + rv * rv).sqrt();
    if !(diameter.hi.is_finite() && diameter.hi <= length) {
        return Ok(false);
    }
    // the image: the surface's Jacobian over the piece's UV box
    match surface.kind_with_nurb(k) {
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Ok(SurfaceKind::ProceduralBlend(_)) | Err(_) => return Ok(false),
        Ok(_) => {}
    }
    let net = match as_b_spline(k, surface) {
        Ok(Some(bs)) => match Nurbs::of(&bs) {
            Ok(n) => n,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(false),
        },
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Ok(None) | Err(_) => return Ok(false),
    };
    let (ur, vr) = (
        Ivl::checked((d0.x - ru).lo, (d0.x + ru).hi),
        Ivl::checked((d0.y - rv).lo, (d0.y + rv).hi),
    );
    let Some(jacobian) = jacobian_over(&net, ur, vr) else {
        return Ok(false);
    };
    let net_max = net
        .pw
        .chunks(4)
        .map(|c| match c {
            [x, y, z, w] => (Ivl::from(*x) / Ivl::from(*w))
                .mag()
                .max((Ivl::from(*y) / Ivl::from(*w)).mag())
                .max((Ivl::from(*z) / Ivl::from(*w)).mag()),
            _ => f64::NAN,
        })
        .fold(0.0_f64, f64::max);
    let trim_max = curve
        .pw
        .chunks(4)
        .map(|c| match c {
            [x, y, ..] => {
                (Ivl::from(*x) * Ivl::from(*x) + Ivl::from(*y) * Ivl::from(*y))
                    .sqrt()
                    .hi
            }
            _ => f64::NAN,
        })
        .fold(0.0_f64, f64::max);
    let image = (diameter * Ivl::from(jacobian)).hi;
    let carried = (Ivl::from(jacobian) * Ivl::from(trim_max)).lo;
    let allowed = (Ivl::from(SLIVER_3D_ULPS * f64::EPSILON) * Ivl::from(net_max.max(carried))).lo;
    Ok(image.is_finite() && image <= allowed)
}

/// The 3D diameter, in units of `ε` times the larger of the surface net's largest
/// coordinate and the trim's largest coordinate carried to 3D, that [`sliver_within`]
/// allows a sliver's image.
pub(crate) const SLIVER_3D_ULPS: f64 = 1024.0;

/// An upper bound on the surface's Jacobian norm `sqrt(|S_u|² + |S_v|²)` over the UV box
/// `ur x vr`, on every polynomial patch of `net` the box meets: the basis functions and
/// their first derivatives over the box clamped to the patch's spans, the homogeneous sums
/// and the quotient rule, in rounded intervals. `None` where the box leaves the knots, a
/// weight's enclosure is not positive, or the bound is not finite.
fn jacobian_over(net: &Nurbs, ur: Ivl, vr: Ivl) -> Option<f64> {
    let us = spans_met(&net.u, net.p, net.nu, ur)?;
    let vs = spans_met(&net.v, net.q, net.nv, vr)?;
    if us.len() * vs.len() > MAX_PATCHES {
        return None;
    }
    let clamp = |x: Ivl, knots: &[f64], k: usize| {
        let (a, b) = (knots.get(k).copied()?, knots.get(k + 1).copied()?);
        Some(Ivl::checked(x.lo.max(a), x.hi.min(b)))
    };
    let mut worst = 0.0_f64;
    for &ku in &us {
        for &kv in &vs {
            let nu = ders_basis(&net.u, ku, net.p, clamp(ur, &net.u, ku)?, 1);
            let nv = ders_basis(&net.v, kv, net.q, clamp(vr, &net.v, kv)?, 1);
            let basis =
                |m: &Vec<Vec<Ivl>>, a: usize, i: usize| m.get(a).and_then(|r| r.get(i)).copied();
            let mut h = [[[Ivl::from(0.0); 4]; 2]; 2];
            for (a, row) in h.iter_mut().enumerate() {
                for (b, hab) in row.iter_mut().enumerate() {
                    for i in 0..=net.p {
                        for j in 0..=net.q {
                            let li = 4
                                * ((ku + i).checked_sub(net.p)? * net.nv
                                    + (kv + j).checked_sub(net.q)?);
                            let c = basis(&nu, a, i)? * basis(&nv, b, j)?;
                            for (e, he) in hab.iter_mut().enumerate() {
                                *he = *he + c * Ivl::from(*net.pw.get(li + e)?);
                            }
                        }
                    }
                }
            }
            let [[h00, h01], [h10, _]] = h;
            let w = h00[3];
            let positive = w.lo > 0.0;
            if !positive {
                return None;
            }
            let point = [h00[0] / w, h00[1] / w, h00[2] / w];
            let [px, py, pz] = point;
            let rate =
                |[x, y, z, wd]: [Ivl; 4]| [(x - wd * px) / w, (y - wd * py) / w, (z - wd * pz) / w];
            let sq = |d: [Ivl; 3]| d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            let norm = (sq(rate(h10)) + sq(rate(h01))).sqrt().hi;
            if !norm.is_finite() {
                return None;
            }
            worst = worst.max(norm);
        }
    }
    Some(worst)
}

/// One piece `[a, b]` of the trim (inside one of its knot spans): `Some(true)` where the
/// bound clears `tol` on every surface piece the trim's points can lie on, `Some(false)`
/// where it does not (or the piece meets too many surface pieces), `None` where the piece's
/// parameter range runs off the surface's knots.
fn piece_clears(curve: &NurbsCurve, net: &Nurbs, a: f64, b: f64, tol: f64) -> Option<bool> {
    let m = 0.5 * (a + b);
    let gap = if b - m > m - a { b - m } else { m - a };
    let h = Ivl::from(round_up(gap));
    let span_t = span(&curve.u, curve.p, curve.n, m);
    let Ok(d) = curve_derivs_over(curve, Ivl::from(m), curve.p, Some(span_t)) else {
        return Some(false);
    };
    // the trim's power coefficients in s: D_j h^j / j!
    let (mut cu, mut cv) = (Poly::new(), Poly::new());
    let mut scale = Ivl::from(1.0);
    for (j, dj) in d.iter().enumerate() {
        if j > 0 {
            scale = scale * h / Ivl::from(j as f64);
        }
        cu.push(dj.x * scale);
        cv.push(dj.y * scale);
    }
    let (ur, vr) = (range(&cu), range(&cv));
    let us = spans_met(&net.u, net.p, net.nu, ur)?;
    let vs = spans_met(&net.v, net.q, net.nv, vr)?;
    if us.len() * vs.len() > MAX_PATCHES {
        return Some(false);
    }
    for &ku in &us {
        for &kv in &vs {
            if !patch_clears(net, ku, kv, &cu, &cv, ur, vr, tol) {
                return Some(false);
            }
        }
    }
    Some(true)
}

/// The non-empty knot spans `k` (`U[k] < U[k+1]`, `p <= k < n`) that `x` meets; `None`
/// where `x` passes the knots' range by more than `KNOT_RANGE_SLACK` or is not finite.
fn spans_met(knots: &[f64], p: usize, n: usize, x: Ivl) -> Option<Vec<usize>> {
    let (first, last) = (knots.get(p).copied()?, knots.get(n).copied()?);
    let big = first.abs().max(last.abs()).max(1.0);
    let slack = KNOT_RANGE_SLACK * big;
    // (written to accept: a NaN end fails)
    let inside = x.lo >= first - slack && x.hi <= last + slack && x.lo <= x.hi;
    if !inside {
        return None;
    }
    let mut out = Vec::new();
    for k in p..n {
        let (k0, k1) = (knots.get(k).copied()?, knots.get(k + 1).copied()?);
        if k0 >= k1 {
            continue;
        }
        // the end spans hold the slack past the knots
        let lo_ok = x.hi >= k0 || (k == p && x.hi >= first - slack);
        let hi_ok = x.lo <= k1 || (k + 1 == n && x.lo <= last + slack);
        if lo_ok && hi_ok {
            out.push(k);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Whether the bound clears `tol` with the surface's polynomial piece `(ku, kv)` composed
/// with the trim piece `(cu, cv)` (power coefficients in `s`; `ur`, `vr` their ranges).
#[allow(clippy::too_many_arguments)]
fn patch_clears(
    net: &Nurbs,
    ku: usize,
    kv: usize,
    cu: &Poly,
    cv: &Poly,
    ur: Ivl,
    vr: Ivl,
    tol: f64,
) -> bool {
    let clamp = |x: f64, lo: f64, hi: f64| {
        if x < lo {
            lo
        } else if x > hi {
            hi
        } else {
            x
        }
    };
    let (Some(&u_a), Some(&u_b), Some(&v_a), Some(&v_b)) = (
        net.u.get(ku),
        net.u.get(ku + 1),
        net.v.get(kv),
        net.v.get(kv + 1),
    ) else {
        return false;
    };
    let (u0, v0) = (clamp(ur.mid(), u_a, u_b), clamp(vr.mid(), v_a, v_b));
    let nu = ders_basis(&net.u, ku, net.p, Ivl::from(u0), net.p);
    let nv = ders_basis(&net.v, kv, net.q, Ivl::from(v0), net.q);
    let at = |m: &Vec<Vec<Ivl>>, i: usize, j: usize| {
        m.get(i)
            .and_then(|r| r.get(j))
            .copied()
            .unwrap_or_else(Ivl::whole)
    };
    // the homogeneous Taylor coefficients H[a][b] / (a! b!) at (u0, v0)
    let mut taylor = vec![vec![[Ivl::from(0.0); 4]; net.q + 1]; net.p + 1];
    let mut fa = 1.0_f64;
    for (a, row) in taylor.iter_mut().enumerate() {
        if a > 0 {
            fa *= a as f64;
        }
        let mut fb = 1.0_f64;
        for (b, hab) in row.iter_mut().enumerate() {
            if b > 0 {
                fb *= b as f64;
            }
            let mut h = [Ivl::from(0.0); 4];
            for i in 0..=net.p {
                for j in 0..=net.q {
                    let li = 4 * ((ku + i - net.p) * net.nv + (kv + j - net.q));
                    let c = at(&nu, a, i) * at(&nv, b, j);
                    for (e, he) in h.iter_mut().enumerate() {
                        let pw = net.pw.get(li + e).copied().unwrap_or(f64::NAN);
                        *he = *he + c * Ivl::from(pw);
                    }
                }
            }
            // a! and b! are exact for the degrees a surface has (below 2^53)
            let f = Ivl::from(fa) * Ivl::from(fb);
            *hab = h.map(|x| x / f);
        }
    }
    // a point near the image, to take the image about
    let h00 = taylor
        .first()
        .and_then(|r| r.first())
        .copied()
        .unwrap_or([Ivl::whole(); 4]);
    let s0 = [h00[0], h00[1], h00[2]].map(|x| (x / h00[3]).mid());
    // u - u0 and v - v0 in s, and their powers
    let shifted = |c: &Poly, x0: f64| {
        let mut c = c.clone();
        if let Some(c0) = c.first_mut() {
            *c0 = *c0 - Ivl::from(x0);
        }
        c
    };
    let powers = |base: &Poly, n: usize| {
        let mut out = vec![vec![Ivl::from(1.0)]];
        for _ in 0..n {
            let next = out.last().map_or_else(Poly::new, |l| mul(l, base));
            out.push(next);
        }
        out
    };
    let (du, dv) = (
        powers(&shifted(cu, u0), net.p),
        powers(&shifted(cv, v0), net.q),
    );
    // the homogeneous image (Q about s0, W)
    let mut q: [Poly; 3] = [Poly::new(), Poly::new(), Poly::new()];
    let mut w = Poly::new();
    for (a, row) in taylor.iter().enumerate() {
        for (b, hab) in row.iter().enumerate() {
            let (Some(pa), Some(pb)) = (du.get(a), dv.get(b)) else {
                return false;
            };
            let basis = mul(pa, pb);
            for ((qe, &he), &se) in q.iter_mut().zip(hab.iter()).zip(s0.iter()) {
                let coeff = he - Ivl::from(se) * hab[3];
                add_into(qe, &basis, coeff);
            }
            add_into(&mut w, &basis, hab[3]);
        }
    }
    let wd = deriv(&w);
    let n: [Poly; 3] = q.clone().map(|qe| {
        let mut ne = mul(&deriv(&qe), &w);
        add_into(&mut ne, &mul(&qe, &wd), Ivl::from(-1.0));
        ne
    });
    let nd: [Poly; 3] = n.clone().map(|ne| deriv(&ne));
    // N x N': g' x g'' = (N x N') / W^4, the tangential part cancelled in the coefficients
    let cross_of = |i: usize, j: usize| {
        let (Some(ni), Some(nj), Some(ndi), Some(ndj)) = (n.get(i), n.get(j), nd.get(i), nd.get(j))
        else {
            return vec![Ivl::whole()];
        };
        let mut c = mul(ni, ndj);
        add_into(&mut c, &mul(nj, ndi), Ivl::from(-1.0));
        c
    };
    let cross = [cross_of(1, 2), cross_of(2, 0), cross_of(0, 1)];

    // the bounds over s in [-1, 1]
    let w_lo = lower_bound(&w);
    let w_hi = upper_magnitude(&w);
    let norm_hi = |p: &[Poly; 3]| {
        let [x, y, z] = p.clone().map(|c| Ivl::from(upper_magnitude(&c)));
        (x * x + y * y + z * z).sqrt().hi
    };
    let cross_hi = norm_hi(&cross);
    let dir = n.clone().map(|c| c.first().map_or(f64::NAN, |x| x.mid()));
    let dir_len = dir
        .iter()
        .map(|&x| Ivl::from(x) * Ivl::from(x))
        .fold(Ivl::from(0.0), |s, x| s + x)
        .sqrt()
        .hi;
    let mut along = Poly::new();
    for (ne, &de) in n.iter().zip(&dir) {
        add_into(&mut along, ne, Ivl::from(de));
    }
    let n_lo = lower_bound(&along);
    let finite = [w_lo, w_hi, cross_hi, dir_len, n_lo]
        .iter()
        .all(|x| x.is_finite());
    let positive = w_lo > 0.0 && n_lo > 0.0 && dir_len > 0.0;
    if !finite || !positive {
        return false;
    }
    // radius = |g'|^3 / |g' x g''| >= (|N|_lo / W_hi^2)^3 / (|N x N'|_hi / W_lo^4)
    let (w_lo, w_hi) = (Ivl::from(w_lo), Ivl::from(w_hi));
    let speed = Ivl::from(n_lo) / (Ivl::from(dir_len) * w_hi * w_hi);
    let speed_lo = Ivl::from(speed.lo);
    let turn = Ivl::from(cross_hi) / (w_lo * w_lo * w_lo * w_lo);
    let lhs = (speed_lo * speed_lo * speed_lo).lo;
    let rhs = (Ivl::from(tol) * Ivl::from(turn.hi)).hi;
    lhs.is_finite() && rhs.is_finite() && lhs >= rhs
}

/// The product of two polynomials.
fn mul(a: &Poly, b: &Poly) -> Poly {
    if a.is_empty() || b.is_empty() {
        return Poly::new();
    }
    let mut out = vec![Ivl::from(0.0); a.len() + b.len() - 1];
    for (i, &x) in a.iter().enumerate() {
        for (j, &y) in b.iter().enumerate() {
            if let Some(o) = out.get_mut(i + j) {
                *o = *o + x * y;
            }
        }
    }
    out
}

/// `acc += c p`.
fn add_into(acc: &mut Poly, p: &Poly, c: Ivl) {
    if acc.len() < p.len() {
        acc.resize(p.len(), Ivl::from(0.0));
    }
    for (o, &x) in acc.iter_mut().zip(p) {
        *o = *o + c * x;
    }
}

/// The derivative in `s`.
fn deriv(p: &Poly) -> Poly {
    p.iter()
        .enumerate()
        .skip(1)
        .map(|(j, &x)| x * Ivl::from(j as f64))
        .collect()
}

/// The values over `s` in `[-1, 1]`: the constant term widened by the other terms'
/// magnitudes.
fn range(p: &Poly) -> Ivl {
    let rest = p
        .iter()
        .skip(1)
        .fold(Ivl::from(0.0), |s, x| s + Ivl::from(x.mag()));
    let c0 = p.first().copied().unwrap_or_else(|| Ivl::from(0.0));
    Ivl::checked((c0 - rest).lo, (c0 + rest).hi)
}

/// A lower bound of the values over `s` in `[-1, 1]` (NaN where not finite).
fn lower_bound(p: &Poly) -> f64 {
    range(p).lo
}

/// An upper bound of the magnitudes over `s` in `[-1, 1]`.
fn upper_magnitude(p: &Poly) -> f64 {
    p.iter()
        .fold(Ivl::from(0.0), |s, x| s + Ivl::from(x.mag()))
        .hi
}
