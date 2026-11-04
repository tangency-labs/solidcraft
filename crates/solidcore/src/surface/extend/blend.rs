//! A procedural blend extended by its definition ([`Surface::extended_by_distance`] on a
//! [`ProceduralBlend`]): across its section, past a side the definition's t runs across.
//!
//! Not upstream: upstream extends a blend's net as any B-spline's (the base class), which
//! leaves the definition behind. Here the result is again a blend: the same definition, its
//! net continued, and its bound against the definition re-derived for the continued part.
//!
//! **Across (t).** The definition's section is exact in t: the chord (order 0, linear,
//! weight 1) or the ball's circle (order 1, the rational quadratic arc with weights 1,
//! `c = cos(theta / 2)`, 1, `c` in `(0, 1]` for an arc under a half turn). Past a foot, the
//! section continued is the same chord or circle. The net is continued Natural across the
//! side ([`Continuity::Natural`] on the net: on its homogeneous rows, the same rational
//! polynomial continued). Its distance from the definition there, at a fixed s (the map
//! `(s, t) = a + b (p, q)` is separable), follows from the source's bound `eps` in closed
//! form: with `W` the section's weight and `V` the net's (polynomials in t of degrees `d_S`
//! and `d_B`), `E = V (W S) - W (V B)` is a polynomial of degree `m = d_S + d_B` with
//! `|E| <= max W max V eps` on the source's t range `[t0, t1]`; past it, by Chebyshev's
//! extremal property, `|E(t)| <= max |E| T_m(x)`, `x = |2 t - t0 - t1| / (t1 - t0)`, so
//!
//! `|S - B|(t) <= eps T_m(x) (max_[t0, t1] W max V) / (min W min V)`
//!
//! over the extension. `W` is 1 for the chord; for the arc `1 - 2 t (1 - t) (1 - c)`: at
//! most `1 + 2 max(0, t (t - 1))` (1 on `[0, 1]`), at least 1 off `(0, 1)` and above 1/2 on
//! it. `max V` is the source net's largest control weight, `min V` the result's least (its
//! convex hull: every weight positive, as the Natural extension proves). The result's
//! bound is `max(eps, eps G + the extension's rounding)`, the growth `G` rounded up.
//!
//! A G^n section (order 2 and up) has no growth factor here and is refused across.
//!
//! **Along the spine (s).** Past the definition's end station (the side its last or first
//! station, the net on its whole section `t in [0, 1]`, s along u): the definition is
//! continued to a new station `s_new` (its leader and law by their end spans' polynomials,
//! its seeds by a span to the new station, as the construction's seeds are the feet's
//! Hermite curves on the stations), and the net gains that span of the construction's:
//! the stations at the end and at `s_new` solved, the span's Hermite rows
//! ([`end_span_of_definition`], the construction's `NetForm::span`) certified against the definition ([`gn_certify_span`]'s bound), and
//! joined after the source's net at multiplicity 3 (the construction's own joints), scaled
//! to its weights. The source's net is kept bit for bit; the piece's first row is the
//! source's own, so its bound against the span's Hermite rows is the joint's row
//! difference ([`net_distance`]). The result's bound: `max(eps, certificate + joint +
//! three floors + rounding)`. The distance is solved as any extension's
//! ([`Surface::extended_by_distance`]), measured on the net; an ask whose station does not
//! solve, whose foot leaves its support's box, or whose span is not proved, works as one
//! whose weights are not positive: the refusal names the longest that does.
//!
//! [`gn_certify_span`]: crate::blend::fillet_section::certificate::gn_certify_span
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::{keeps, op_err, solve, Continuity, Extended, Local, Made, H, OP, REL};
use crate::blend::fillet_section::recertify::{
    end_span_of_definition, net_distance, precision_floor, DefinitionSpan,
};
use crate::blend::fillet_section::section::{param, param_in};
use crate::blend::fillet_section::station::GnStation;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::{gamma, CPoint, NOW};
use crate::nurbs::srf_basic::Surface as Net;
use crate::surface::procedural_blend::ProceduralBlend;
use crate::surface::side::BoxSide;
use crate::surface::surface::{Surface, SurfaceKind};

/// The blend `pb` extended `distance` past side `side` of `dom` (module docs).
pub(super) fn blend_extended(
    k: &Kernel,
    pb: &ProceduralBlend,
    dom: &Extent2d,
    side: BoxSide,
    distance: f64,
) -> Result<Extended> {
    let (a, b, swapped) = pb.definition_map();
    // the definition's s is the net's u, its t the net's v (the other way round swapped)
    if side.is_u() != swapped {
        return along(k, pb, dom, side, distance);
    }
    let order = pb.order();
    let section_degree = match order {
        0 => 1,
        1 => 2,
        _ => {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a G^n section has no growth factor past its source's t range here",
            })
        }
    };
    let source_net = pb.net.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    let grown = Surface::from(pb.net.clone()).extended_by_distance(
        k,
        dom,
        side,
        Continuity::Natural,
        distance,
    )?;
    let SurfaceKind::BSpline(net) = grown.surface.kind() else {
        return Err(SolidError::port_invariant(
            OP,
            "the net's extension is not a B-spline",
        ));
    };
    let out_net = net.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    let (lo, hi) = if side.is_u() {
        (dom.min().x, dom.max().x)
    } else {
        (dom.min().y, dom.max().y)
    };
    let (ta, tb) = (side.at(dom), if side.is_high() { lo } else { hi });
    let te = side.at(&grown.surface.natural_uv_domain_with_nurb(k)?);
    let t_of = |q: f64| a[1] + b[1] * q;
    let (ts, tf, te) = (t_of(ta), t_of(tb), t_of(te));
    let (t0, t1) = (ts.min(tf), ts.max(tf));
    // (a NaN end fails too)
    if t1.partial_cmp(&t0) != Some(std::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(
            OP,
            "the blend's t range is empty",
        ));
    }
    let net_degree = if side.is_u() {
        source_net.p()
    } else {
        source_net.q()
    };
    let m = section_degree + net_degree;
    let x = (2.0 * te - t0 - t1).abs() / (t1 - t0);
    let cheb = chebyshev(m, x);
    let (w_hi, w_lo) = if order == 0 {
        (1.0, 1.0)
    } else {
        let off = |t: f64| (t * (t - 1.0)).max(0.0);
        let hi = 1.0 + 2.0 * off(t0).max(off(t1));
        let (lo_t, hi_t) = (ts.min(te), ts.max(te));
        let lo = if lo_t < 1.0 && hi_t > 0.0 { 0.5 } else { 1.0 };
        (hi, lo)
    };
    let (v_hi, _) = weights(source_net);
    let (_, v_lo) = weights(out_net);
    if !(v_lo > 0.0 && v_hi.is_finite()) {
        return Err(SolidError::port_invariant(
            OP,
            "a weight of the net is not positive",
        ));
    }
    let growth = cheb * (w_hi * v_hi) / (w_lo * v_lo) * (1.0 + gamma(4 * m + 16));
    let eps = pb.bound();
    let bound = eps.max(eps * growth + grown.extension) * (1.0 + gamma(2));
    if !bound.is_finite() {
        return Err(SolidError::ExtendRefused {
            operation: OP,
            refusal: super::ExtendRefusal::NotExact,
            distance: None,
        });
    }
    let mut out = ProceduralBlend {
        net: net.clone(),
        def: pb.def.clone(),
    };
    out.def.bound = bound;
    out.renew();
    Ok(Extended {
        surface: Surface::from(out),
        ..grown
    })
}

/// `T_m(x)` for `x >= 1` (`|x|` taken), by its recurrence (every term positive and
/// increasing there), rounded up.
fn chebyshev(m: usize, x: f64) -> f64 {
    let x = x.abs().max(1.0);
    let (mut prev, mut cur) = (1.0_f64, x);
    if m == 0 {
        return 1.0;
    }
    for _ in 1..m {
        let next = 2.0 * x * cur - prev;
        prev = cur;
        cur = next;
    }
    cur * (1.0 + gamma(3 * m + 3))
}

/// The largest and least control weight of a net (1 for a polynomial one).
fn weights(net: &Net) -> (f64, f64) {
    let (mut hi, mut lo) = (0.0_f64, f64::INFINITY);
    for c in net.cpts().iter().flatten() {
        let w = if c.w == NOW { 1.0 } else { c.w };
        hi = hi.max(w);
        lo = lo.min(w);
    }
    (hi, lo)
}

/// Along the spine: the definition continued past its end station and its net by one more
/// span of the construction's (module docs).
fn along(
    k: &Kernel,
    pb: &ProceduralBlend,
    dom: &Extent2d,
    side: BoxSide,
    distance: f64,
) -> Result<Extended> {
    let (a, b, swapped) = pb.definition_map();
    let t_of = |q: f64| a[1] + b[1] * q;
    if swapped || t_of(dom.min().y) != 0.0 || t_of(dom.max().y) != 1.0 {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "a blend is extended along its spine only on its whole section, s along u",
        });
    }
    let def = pb.definition();
    let sigma = def
        .seeds
        .first()
        .ok_or_else(|| SolidError::sm_err(OP))?
        .knots(None)?
        .0;
    let s_side = a[0] + b[0] * side.at(dom);
    // outward from the side, s grows (`up`) or falls
    let up = side.is_high() == (b[0] > 0.0);
    let end = if up { sigma.last() } else { sigma.first() };
    if end.copied() != Some(s_side) {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "the side is not the definition's end station",
        });
    }
    let source_net = pb.net.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    let source = Local::of(source_net, side)?;
    if source.p != 3 {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "a blend's net is cubic along its spine",
        });
    }
    let target = pb.bound();
    // the definition and its end span for the ask e (in the net's parameter)
    let piece_of = |e: f64| -> Result<Option<(ProceduralBlend, DefinitionSpan)>> {
        let ds = e * b[0].abs();
        let s_new = if up { s_side + ds } else { s_side - ds };
        let next = continued_definition(pb, s_new, up)?;
        // a station that does not solve, a foot off its support or a span not proved: an
        // ask that does not work, as a weight not positive
        let span = match end_span_of_definition(k, &next, up, target) {
            Ok(span) if span.certified.is_finite() && feet_on(k, &next, &span)? => span,
            Ok(_) | Err(SolidError::InvalidInput { .. } | SolidError::Upstream { .. }) => {
                return Ok(None)
            }
            Err(e) => return Err(e),
        };
        Ok(Some((next, span)))
    };
    let made = |e: f64| -> Result<Option<Made>> {
        let Some((_, span)) = piece_of(e)? else {
            return Ok(None);
        };
        let (ext, piece, scale) = piece_rows(&source, &span, up)?;
        // the piece's own bound waits for the met ask: here only that it is finite
        if !piece_distance(&piece, scale)?.is_finite() {
            return Ok(None);
        }
        let mut grown = source.joined(ext, &[source.end()? + e; 4])?;
        // the scaled points' rounding, one product each
        grown.err = gamma(1) * grown.size();
        let bound = grown.bound();
        let out = grown.surface(source_net, side)?;
        Ok(Some((
            Some(Surface::from(BSplineSurface::from_gw_nurb(&out))),
            bound,
        )))
    };
    let across =
        |m: &Option<Surface>| match m.as_ref().map(|s| s.length_across_side(k, dom, side, REL)) {
            None | Some(Err(SolidError::NotConverged { .. })) => Ok(None),
            Some(Ok(a)) => Ok(Some(a)),
            Some(Err(e)) => Err(e),
        };
    let ((surface, extension), enclosure) = solve(&made, &across, &source, distance)?;
    let surface = surface.ok_or_else(op_err)?;
    let SurfaceKind::BSpline(net) = surface.kind() else {
        return Err(SolidError::port_invariant(
            OP,
            "the extension is not a B-spline",
        ));
    };
    let out_net = net.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    if !keeps(source_net, out_net, side)? {
        return Err(SolidError::port_invariant(
            OP,
            "the extension moved the source's net",
        ));
    }
    // the met ask's definition again (the same solve and certificate, deterministic), its
    // seeds the feet's Hermite curves on its stations, and the piece's bound against it
    let grown_end = Local::of(out_net, side)?.end()?;
    let e = grown_end - source.end()?;
    let (mut next, span) = piece_of(e)?.ok_or_else(op_err)?;
    hermite_seeds(&mut next, &span.stations, up)?;
    let (_, rows, scale) = piece_rows(&source, &span, up)?;
    let piece = span.certified + piece_distance(&rows, scale)?;
    let floor = 3.0 * precision_floor(k, next.supports(), next.order(), net)?;
    let mut out = ProceduralBlend {
        net: net.clone(),
        def: next.def,
    };
    out.def.bound = pb.bound().max(piece + floor + extension) * (1.0 + gamma(2));
    out.renew();
    Ok(Extended {
        surface: Surface::from(out),
        source: 0.0,
        extension,
        across: enclosure,
        dom: *dom,
        sides: BoxSide::ALL,
    })
}

/// `|B - H|` over the piece: `piece[j]` the net's rows along s at across index j (the
/// source's joint row, then the scaled span rows) and the span's own rows (scaled by
/// `scale`, the same surface), as [`net_distance`] bounds two nets.
fn piece_distance(piece: &[(Vec<H>, Vec<H>)], scale: f64) -> Result<f64> {
    let hpt = |x: &H| (Vec3::new(x[0], x[1], x[2]), x[3]);
    let len = piece.first().map_or(0, |(b, _)| b.len());
    let b: Vec<Vec<(Vec3, f64)>> = (0..len)
        .map(|i| {
            piece
                .iter()
                .filter_map(|(b, _)| b.get(i))
                .map(hpt)
                .collect()
        })
        .collect();
    let h: Vec<Vec<(Vec3, f64)>> = (0..len)
        .map(|i| {
            piece
                .iter()
                .filter_map(|(_, h)| h.get(i))
                .map(|x| hpt(&x.map(|c| scale * c)))
                .collect()
        })
        .collect();
    net_distance(&b, &h)
}

/// A curve of the definition (its leader, law or a seed) continued to `s_new` past its
/// high end (`up`) or its low end: its end span's polynomial continued
/// ([`Local::natural`]), its own points and knots kept. A curve already reaching `s_new`
/// is itself.
fn continued_curve(c: &BSplineCurve, s_new: f64, up: bool) -> Result<BSplineCurve> {
    let (knots, mult) = c.knots(None)?;
    let (Some(&lo), Some(&hi)) = (knots.first(), knots.last()) else {
        return Err(op_err());
    };
    if (up && s_new <= hi) || (!up && s_new >= lo) {
        return Ok(c.clone());
    }
    let p = c.degree()?;
    let cps = c.control_points()?;
    let rational = cps.iter().any(|x| x.w != NOW);
    let mut all = Vec::new();
    for (&k, &m) in knots.iter().zip(&mult) {
        all.extend(std::iter::repeat_n(k, m));
    }
    let row: Vec<H> = cps
        .iter()
        .map(|x| {
            if x.w == NOW {
                [x.x, x.y, x.z, 1.0]
            } else {
                [x.x, x.y, x.z, x.w]
            }
        })
        .collect();
    let mut l = Local {
        rows: vec![row],
        knots: all,
        p,
        rational,
        err: 0.0,
    };
    if !up {
        l.reverse();
    }
    let e = if up { s_new - hi } else { lo - s_new };
    let mut grown = l.natural(e)?;
    if !up {
        grown.reverse();
    }
    let row = grown.rows.into_iter().next().ok_or_else(op_err)?;
    let pw: Vec<CPoint> = row
        .iter()
        .map(|x| CPoint::new(x[0], x[1], x[2], if rational { x[3] } else { NOW }))
        .collect();
    let curve = crate::nurbs::crv_basic::Curve::new(pw, p, grown.knots)?;
    let mut out = BSplineCurve::from_gw_nurb(c.dim(), &curve);
    out.set_out_of_bounds_enabled(true);
    Ok(out)
}

/// The definition continued to `s_new` past its end station (`up`: its last), on the
/// source's net: its leader and law continued ([`continued_curve`]), and its seeds given a
/// station at `s_new`, their end span continued (the stations' Newton guesses there).
fn continued_definition(pb: &ProceduralBlend, s_new: f64, up: bool) -> Result<ProceduralBlend> {
    let mut next = pb.clone();
    let d = &mut next.def;
    d.leader = continued_curve(&d.leader, s_new, up)?;
    if let Some(l) = d.law.as_ref() {
        d.law = Some(continued_curve(l, s_new, up)?);
    }
    for sd in &mut d.seeds {
        *sd = continued_curve(sd, s_new, up)?;
    }
    next.renew();
    Ok(next)
}

/// The seeds as the feet's Hermite curves on the stations (as the construction makes
/// them): each seed's new span's three points from the end span's stations `[lo, hi]` (in
/// s), its own points before kept.
fn hermite_seeds(next: &mut ProceduralBlend, stations: &[GnStation], up: bool) -> Result<()> {
    let (Some(lo), Some(hi)) = (stations.first(), stations.get(1)) else {
        return Err(op_err());
    };
    let h = hi.s - lo.s;
    for (f, sd) in next.def.seeds.iter_mut().enumerate() {
        let v = |st: &GnStation| Vec3::new(param(st, 2 * f, 0, 0), param(st, 2 * f + 1, 0, 0), 0.0);
        let d = |st: &GnStation| Vec3::new(param(st, 2 * f, 1, 0), param(st, 2 * f + 1, 1, 0), 0.0);
        let d_in = |st: &GnStation| {
            Vec3::new(
                param_in(st, 2 * f, 1, 0),
                param_in(st, 2 * f + 1, 1, 0),
                0.0,
            )
        };
        let mut cps: Vec<CPoint> = sd.control_points()?.to_vec();
        let len = cps.len();
        // the span's inner points and its new end: the seed's own joint point kept
        let new = if up {
            [
                v(lo) + d(lo) * (h / 3.0),
                v(hi) - d_in(hi) * (h / 3.0),
                v(hi),
            ]
        } else {
            [
                v(lo),
                v(lo) + d(lo) * (h / 3.0),
                v(hi) - d_in(hi) * (h / 3.0),
            ]
        };
        let first = if up {
            len.checked_sub(3).ok_or_else(op_err)?
        } else {
            0
        };
        let at = [first, first + 1, first + 2];
        for (i, x) in at.into_iter().zip(new) {
            let c = cps.get_mut(i).ok_or_else(op_err)?;
            *c = CPoint::new(x.x, x.y, c.z, c.w);
        }
        let (knots, mult) = sd.knots(None)?;
        let mut all = Vec::new();
        for (&k, &m) in knots.iter().zip(&mult) {
            all.extend(std::iter::repeat_n(k, m));
        }
        let curve = crate::nurbs::crv_basic::Curve::new(cps, sd.degree()?, all)?;
        let mut out = BSplineCurve::from_gw_nurb(sd.dim(), &curve);
        out.set_out_of_bounds_enabled(true);
        *sd = out;
    }
    Ok(())
}

/// The span's rows joined to the source past its side: per across index j the three
/// points after the joint, scaled to the source's weights (the source's net is the
/// stations' rows scaled by one factor, which the joint's weights give); the piece's net
/// (the source's joint row, then those) beside the span's own rows; and the scale.
#[allow(clippy::type_complexity)]
fn piece_rows(
    source: &Local,
    span: &DefinitionSpan,
    up: bool,
) -> Result<(Vec<Vec<H>>, Vec<(Vec<H>, Vec<H>)>, f64)> {
    let ld = span.ld;
    if source.rows.len() != ld {
        return Err(SolidError::port_invariant(
            OP,
            "the net is not the section's width",
        ));
    }
    // the span's row i outward from the joint at across index j
    let row = |i: usize, j: usize| -> Option<H> {
        let i = if up { i } else { 3_usize.checked_sub(i)? };
        let at = i.checked_mul(ld)?.checked_add(j)?;
        let (p, w) = (span.homog.get(at)?, span.weights.get(at)?);
        Some([p.x, p.y, p.z, *w])
    };
    let (Some(joint), Some(src)) = (row(0, 0), source.rows.first().and_then(|r| r.last())) else {
        return Err(op_err());
    };
    let scale = src[3] / joint[3];
    let mut ext = Vec::new();
    let mut piece = Vec::new();
    for (j, srow) in source.rows.iter().enumerate() {
        let (Some(&sj), Some(hj)) = (srow.last(), row(0, j)) else {
            return Err(op_err());
        };
        let (mut q, mut h, mut b) = (Vec::new(), vec![hj], vec![sj]);
        for i in 1..4 {
            let x = row(i, j).ok_or_else(op_err)?;
            let y = x.map(|c| scale * c);
            q.push(y);
            h.push(x);
            b.push(y);
        }
        ext.push(q);
        piece.push((b, h));
    }
    Ok((ext, piece, scale))
}

/// Whether every foot of the span's stations is on its support's natural box.
fn feet_on(k: &Kernel, pb: &ProceduralBlend, span: &DefinitionSpan) -> Result<bool> {
    for (f, support) in pb.supports().into_iter().enumerate() {
        let b = support.natural_uv_domain_with_nurb(k)?;
        for st in &span.stations {
            let (u, v) = (param(st, 2 * f, 0, 0), param(st, 2 * f + 1, 0, 0));
            if !(u >= b.min().x && u <= b.max().x && v >= b.min().y && v <= b.max().y) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
