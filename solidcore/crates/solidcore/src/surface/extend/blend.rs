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
//! A G^n section (order `n >= 2`) is the same argument with `d_S = 3 n + 1` and its weight
//! `W = A B^n C^n` (`A = (1 - t)^(n+1) + t^(n+1)`, `B = 1 - w + w t`, `C = 1 - w t`), bounded
//! in closed form over a proved range of its thumbweight `w`: a constant one, or the equal
//! area one's enclosure from its certificate's interval Newton roots (the stations' own w's
//! padded would be no proof). `A` grows with `|t - 1/2|`; `B` and `C` are bilinear in `(t,
//! w)`, their extremes at the corners of the (t range) x (w range) box. A corner past which
//! `B` or `C` is not positive is a weight that vanishes: refused.
//!
//! **Along the spine (s).** Past the definition's end station (the side its last or first
//! station, the net on its whole section `t in [0, 1]`, s along u): the definition is
//! continued to a new station `s_new` (its leader and law by their end spans' polynomials,
//! its seeds by a span to the new station, as the construction's seeds are the feet's
//! Hermite curves on the stations), and the net gains that span of the construction's:
//! the stations at the end and at `s_new` solved, the span's Hermite rows
//! ([`span_of_definition`], the construction's `NetForm::span`) certified against the
//! definition ([`gn_certify_span`]'s bound), and joined after the source's net at
//! multiplicity 5 (the construction's own joints), scaled to its weights. A side INSIDE the
//! spine (a piece of the blend cut across it) runs the definition's spans already there
//! first: each span the extension crosses solved and certified the same way, the part of
//! it past the side (or up to `s_new`) its Hermite rows' quintic Bezier cut there (blossoms),
//! each part joined at multiplicity 5 and scaled to the row before it; then past the end
//! as above. The source's net is kept bit for bit; each part's first row is the row before
//! it, so its bound against its span's rows is the joint's row difference
//! ([`net_distance`]). The result's bound: `max(eps, the largest part's certificate +
//! joint, + three floors + rounding)`. The distance is solved as any extension's
//! ([`Surface::extended_by_distance`]), measured on the net; an ask whose station does not
//! solve, whose foot leaves a B-spline support's box (an analytic support continued is still itself), or whose span is not proved, works as one
//! whose weights are not positive: the refusal names the longest that does.
//!
//! [`gn_certify_span`]: crate::blend::fillet_section::certificate::gn_certify_span
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::{keeps, op_err, solve, Continuity, Extended, Local, Made, H, OP, REL};
use crate::blend::fillet_section::continuation::{
    growth, net_weights, section_degree, section_weight_range,
};
use crate::blend::fillet_section::form::{hermite_points_sided, HERMITE_DEGREE};
use crate::blend::fillet_section::recertify::recertify_with_thumbweight;
use crate::blend::fillet_section::recertify::{
    net_distance, precision_floor, span_of_definition, DefinitionSpan,
};
use crate::blend::fillet_section::section::{param, uv_jet, GN_THUMBWEIGHT_EQUAL_AREA};
use crate::blend::fillet_section::series::HERMITE;
use crate::blend::fillet_section::station::GnStation;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::{gamma, CPoint, NOW};
use crate::surface::implicit::AnalyticSupport;
use crate::surface::procedural_blend::ProceduralBlend;
use crate::surface::side::BoxSide;
use crate::surface::surface::{Surface, SurfaceKind};

/// The blend `pb` extended `distance` past side `side` of `dom` (module docs).
pub(super) fn blend_extended(
    k: &Kernel,
    pb: &ProceduralBlend,
    dom: &Extent2d,
    at: &Extent2d,
    side: BoxSide,
    distance: f64,
) -> Result<Option<Extended>> {
    let (a, b, swapped) = pb.definition_map();
    // the definition's s is the net's u, its t the net's v (the other way round swapped)
    if side.is_u() != swapped {
        return along(k, pb, dom, at, side, distance);
    }
    let order = pb.order();
    // the section's degree in t: the chord's 1, the circle's 2, a G^n section's 3 n + 1
    let section_deg = section_degree(order);
    let source_net = pb
        .net
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
    // a distance below the net side's resolution: none (the blend as it is)
    let Some(grown) = Surface::from(pb.net.clone()).extended_by_distance(
        k,
        dom,
        side,
        Continuity::Natural,
        distance,
    )?
    else {
        return Ok(None);
    };
    let SurfaceKind::BSpline(net) = grown.surface.kind() else {
        return Err(SolidError::port_invariant(
            OP,
            "the net's extension is not a B-spline",
        ));
    };
    let out_net = net
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
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
    let m = section_deg + net_degree;
    let x = (2.0 * te - t0 - t1).abs() / (t1 - t0);
    let (w_hi, w_lo) = section_weight_range(order, [t0, t1], [ts.min(te), ts.max(te)], || {
        thumbweight(k, pb)
    })?;
    let (v_hi, _) = net_weights(source_net);
    let (_, v_lo) = net_weights(out_net);
    if !(v_lo > 0.0 && v_hi.is_finite()) {
        return Err(SolidError::port_invariant(
            OP,
            "a weight of the net is not positive",
        ));
    }
    let growth = growth(m, x, (w_hi, w_lo), (v_hi, v_lo));
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
    Ok(Some(Extended {
        surface: Surface::from(out),
        ..grown
    }))
}

/// The thumbweight of `pb`'s G^n section over its face, `[lo, hi]`: a constant one, or the
/// equal area one's proved enclosure from its certificate's interval Newton roots
/// ([`recertify_with_thumbweight`]).
fn thumbweight(k: &Kernel, pb: &ProceduralBlend) -> Result<[f64; 2]> {
    let tw = pb.definition().thumbweight;
    if tw != GN_THUMBWEIGHT_EQUAL_AREA {
        return Ok([tw, tw]);
    }
    recertify_with_thumbweight(k, pb)?
        .1
        .ok_or(SolidError::Unsupported {
            operation: OP,
            reason: "the equal area thumbweight is not enclosed over the blend",
        })
}

/// Along the spine: the definition continued past its end station and its net by one more
/// span of the construction's (module docs).
fn along(
    k: &Kernel,
    pb: &ProceduralBlend,
    dom: &Extent2d,
    at: &Extent2d,
    side: BoxSide,
    distance: f64,
) -> Result<Option<Extended>> {
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
        .ok_or_else(|| SolidError::surface_failed(OP, "the definition has no seed curve"))?
        .knots(None)?
        .0;
    let s_side = a[0] + b[0] * side.at(dom);
    // outward from the side, s grows (`up`) or falls
    let up = side.is_high() == (b[0] > 0.0);
    // the side on the definition's spine: at its end station, or inside it (extended by
    // the definition's spans already there, then past its end)
    let (Some(&first), Some(&last)) = (sigma.first(), sigma.last()) else {
        return Err(SolidError::surface_failed(
            OP,
            "the seed curve has no knots",
        ));
    };
    let end = if up { last } else { first };
    if !(s_side == end || (s_side > first && s_side < last)) {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "the side is not on the definition's spine",
        });
    }
    let source_net = pb
        .net
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
    let source = Local::of(source_net, side)?;
    if source.p != HERMITE_DEGREE {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "a blend's net is not of its Hermite spans' degree along its spine",
        });
    }
    let target = pb.bound();
    let rate = b[0].abs();
    // the shortest span the net's knot fixup keeps where two pieces join, as at the side
    let least = source.least_step()?;
    // the definition and its spans from the side to s_new for the ask e (in the net's
    // parameter), outward, each with its part `[x, y]` in s
    let pieces_of = |e: f64| -> Result<Option<(ProceduralBlend, Vec<Piece>)>> {
        let ds = e * rate;
        let s_new = if up { s_side + ds } else { s_side - ds };
        let next = continued_definition(pb, s_new, up)?;
        let sig = next
            .definition()
            .seeds
            .first()
            .ok_or_else(op_err)?
            .knots(None)?
            .0;
        let (lo, hi) = if up { (s_side, s_new) } else { (s_new, s_side) };
        let mut pieces = Vec::new();
        for (idx, (sa, sb)) in sig.iter().zip(sig.iter().skip(1)).enumerate() {
            let (x, y) = (sa.max(lo), sb.min(hi));
            if y <= x {
                continue;
            }
            // a station that does not solve, a foot off its support or a span not proved:
            // an ask that does not work, as a weight not positive
            let span = match span_of_definition(k, &next, idx, target) {
                Ok(span) if span.certified.is_finite() && feet_on(k, &next, &span)? => span,
                Ok(_) | Err(SolidError::InvalidInput { .. }) => return Ok(None),
                Err(e) if e.upstream_status().is_some() => return Ok(None),
                Err(e) => return Err(e),
            };
            let length = (y - x) / rate;
            // a piece clipped shorter than a span the knot fixup keeps (the side just before a
            // station, or the ask just past one) would join on a knot it merges or spreads
            // (the source's net not kept): an ask that does not work, as a weight not positive
            if length < least {
                return Ok(None);
            }
            pieces.push(Piece {
                span,
                part: [(x - sa) / (sb - sa), (y - sa) / (sb - sa)],
                length,
            });
        }
        if !up {
            pieces.reverse();
        }
        if pieces.is_empty() {
            return Ok(None);
        }
        Ok(Some((next, pieces)))
    };
    let made = |e: f64| -> Result<Option<Made>> {
        let Some((_, pieces)) = pieces_of(e)? else {
            return Ok(None);
        };
        let (ext, rows) = pieces_rows(&source, &pieces, up)?;
        // the pieces' own bounds wait for the met ask: here only that they are finite
        for (piece, scale) in &rows {
            if !piece_distance(piece, *scale)?.is_finite() {
                return Ok(None);
            }
        }
        let grown = source.joined(ext, &joints(source.end()?, e, &pieces))?;
        let mut grown = grown;
        // the scaled points' rounding, one product each, and each part's subdivision (three
        // levels of convex combinations, the part's ends a few more)
        grown.err = gamma(16) * grown.size();
        let bound = grown.bound();
        let out = grown.surface(side)?;
        Ok(Some((
            Some(Surface::from(BSplineSurface::from_gw_nurb(&out))),
            bound,
        )))
    };
    let across =
        |m: &Option<Surface>| match m.as_ref().map(|s| s.length_across_side(k, at, side, REL)) {
            None | Some(Err(SolidError::NotConverged { .. })) => Ok(None),
            Some(Ok(a)) => Ok(Some(a)),
            Some(Err(e)) => Err(e),
        };
    // a distance below the side's resolution: none (the blend as it is)
    let Some(((surface, extension), enclosure)) = solve(&made, &across, &source, distance)? else {
        return Ok(None);
    };
    let surface = surface.ok_or_else(op_err)?;
    let SurfaceKind::BSpline(net) = surface.kind() else {
        return Err(SolidError::port_invariant(
            OP,
            "the extension is not a B-spline",
        ));
    };
    let out_net = net
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
    if !keeps(source_net, out_net, side)? {
        return Err(SolidError::port_invariant(
            OP,
            "the extension moved the source's net",
        ));
    }
    // the met ask's definition again (the same solves and certificates, deterministic);
    // past the end station its seeds the feet's Hermite curves on its new span's
    // stations; each piece's bound against its span
    let grown_end = Local::of(out_net, side)?.end()?;
    let e = grown_end - source.end()?;
    let (mut next, pieces) = pieces_of(e)?.ok_or_else(op_err)?;
    let past_end = if up {
        s_side + e * rate > last
    } else {
        s_side - e * rate < first
    };
    if past_end {
        let outer = pieces.last().ok_or_else(op_err)?;
        hermite_seeds(&mut next, &outer.span.stations, up)?;
    }
    let (_, rows) = pieces_rows(&source, &pieces, up)?;
    let mut piece = 0.0_f64;
    for (p, (r, scale)) in pieces.iter().zip(&rows) {
        piece = piece.max(p.span.certified + piece_distance(r, *scale)?);
    }
    let floor = 3.0 * precision_floor(k, next.supports(), next.order(), net)?;
    let mut out = ProceduralBlend {
        net: net.clone(),
        def: next.def,
    };
    out.def.bound = pb.bound().max(piece + floor + extension) * (1.0 + gamma(2));
    out.renew();
    Ok(Some(Extended {
        surface: Surface::from(out),
        source: 0.0,
        extension,
        across: enclosure,
        dom: *dom,
        sides: BoxSide::ALL,
    }))
}

/// One span of a definition an extension along its spine runs: the span, the part of it
/// it runs (`[x, y]` of its own parameter in `[0, 1]`, in s's direction) and that part's
/// length in the net's parameter.
struct Piece {
    span: DefinitionSpan,
    part: [f64; 2],
    length: f64,
}

/// The knots after the source's joint for `pieces` from `end` by `e` in all: each part's
/// end at multiplicity [`HERMITE_DEGREE`] (the construction's own joints), the last
/// clamped (one more), at exactly `end + e`.
fn joints(end: f64, e: f64, pieces: &[Piece]) -> Vec<f64> {
    let mut knots = Vec::new();
    let mut at = end;
    let n = pieces.len();
    for (i, p) in pieces.iter().enumerate() {
        if i + 1 == n {
            knots.extend([end + e; HERMITE_DEGREE + 1]);
        } else {
            at += p.length;
            knots.extend([at; HERMITE_DEGREE]);
        }
    }
    knots
}

/// The Bezier `p` of degree [`HERMITE_DEGREE`] over `[t0, t1]` of its parameter: its
/// blossoms `B(t0^(d-i), t1^i)`.
fn sub_bezier(p: [H; HERMITE], t0: f64, t1: f64) -> [H; HERMITE] {
    let blossom = |i1: usize| -> H {
        let mut q = p;
        for level in 0..HERMITE_DEGREE {
            // the first `HERMITE_DEGREE - i1` arguments t0, the rest t1
            let t = if level < HERMITE_DEGREE - i1 { t0 } else { t1 };
            for i in 0..HERMITE_DEGREE - level {
                if let (Some(&a), Some(&b)) = (q.get(i), q.get(i + 1)) {
                    if let Some(x) = q.get_mut(i) {
                        *x = super::lin(1.0 - t, a, t, b);
                    }
                }
            }
        }
        q.first().copied().unwrap_or([f64::NAN; 4])
    };
    core::array::from_fn(blossom)
}

/// The extension's rows past the source's joint for `pieces` (outward), each part's rows
/// scaled to the weight of the row before it (the source's joint row for the first), and
/// for each piece its rows against its span's ([`piece_distance`]) with its scale.
#[allow(clippy::type_complexity)]
fn pieces_rows(
    source: &Local,
    pieces: &[Piece],
    up: bool,
) -> Result<(Vec<Vec<H>>, Vec<(Vec<(Vec<H>, Vec<H>)>, f64)>)> {
    let mut ext: Vec<Vec<H>> = vec![Vec::new(); source.rows.len()];
    let mut out = Vec::new();
    // the row before each piece, across: the source's joint row first
    let mut before: Vec<H> = source
        .rows
        .iter()
        .map(|r| r.last().copied().ok_or_else(op_err))
        .collect::<Result<_>>()?;
    for p in pieces {
        let ld = p.span.ld;
        if source.rows.len() != ld {
            return Err(SolidError::port_invariant(
                OP,
                "the net is not the section's width",
            ));
        }
        // the span's part at across index j, outward from the joint
        let part = |j: usize| -> Result<[H; HERMITE]> {
            let mut c = [[0.0; 4]; HERMITE];
            for (i, ci) in c.iter_mut().enumerate() {
                let at = i
                    .checked_mul(ld)
                    .and_then(|x| x.checked_add(j))
                    .ok_or_else(op_err)?;
                let (q, w) = (
                    p.span.homog.get(at).ok_or_else(op_err)?,
                    p.span.weights.get(at).ok_or_else(op_err)?,
                );
                *ci = [q.x, q.y, q.z, *w];
            }
            let mut sub = sub_bezier(c, p.part[0], p.part[1]);
            if !up {
                sub.reverse();
            }
            Ok(sub)
        };
        let (Some(b0), Ok(h0)) = (before.first(), part(0)) else {
            return Err(op_err());
        };
        let scale = b0[3] / h0[0][3];
        let mut rows = Vec::new();
        for (j, (e, bj)) in ext.iter_mut().zip(before.iter_mut()).enumerate() {
            let h = part(j)?;
            let (mut hs, mut bs) = (vec![h[0]], vec![*bj]);
            for x in h.iter().skip(1) {
                let y = x.map(|c| scale * c);
                e.push(y);
                hs.push(*x);
                bs.push(y);
            }
            *bj = *bs.last().ok_or_else(op_err)?;
            rows.push((bs, hs));
        }
        out.push((rows, scale));
    }
    Ok((ext, out))
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
        err_w: 0.0,
        q: 0,
        frame: None,
        across: Vec::new(),
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
/// them): each seed's new span's points past its joint from the end span's stations
/// `[lo, hi]` (in s), its own points before kept (a seed of a lower degree raised to the
/// spans' first).
fn hermite_seeds(next: &mut ProceduralBlend, stations: &[GnStation], up: bool) -> Result<()> {
    let (Some(lo), Some(hi)) = (stations.first(), stations.get(1)) else {
        return Err(op_err());
    };
    for (f, sd) in next.def.seeds.iter_mut().enumerate() {
        // the span's Bezier points, its first the seed's own joint point (kept)
        let span = hermite_points_sided(&[lo.s, hi.s], &[uv_jet(lo, f), uv_jet(hi, f)]);
        // a seed of a lower degree (one the definition was given, not the construction's)
        // raised to the spans' degree first: the same curve, its joints Bezier joints still
        let p = sd.degree()?;
        if p > HERMITE_DEGREE {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a seed's degree is past the Hermite spans'",
            });
        }
        if p < HERMITE_DEGREE {
            sd.degree_elevate(HERMITE_DEGREE)?;
        }
        let mut cps: Vec<CPoint> = sd.control_points()?.to_vec();
        let len = cps.len();
        // the span's inner points and its new end
        let (first, new) = if up {
            (
                len.checked_sub(HERMITE_DEGREE).ok_or_else(op_err)?,
                span.get(1..).ok_or_else(op_err)?,
            )
        } else {
            (0, span.get(..HERMITE_DEGREE).ok_or_else(op_err)?)
        };
        for (i, x) in (first..first + HERMITE_DEGREE).zip(new) {
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

/// Whether every foot of the span's stations is on its support: on its natural box, or
/// anywhere on an analytic support (plane, cone, sphere, torus). The stations are solved on
/// the support's net continued past its knots ([`crate::blend::fillet_section::station`]);
/// an analytic's net continued is still on the analytic (its implicit equation composed
/// with the homogeneous net is a polynomial identity, so it holds on the continuation,
/// wherever the weight is not zero), so a foot off its box is still on the surface. A
/// B-spline's continuation is no longer its face's surface: a foot off its box is not on it.
fn feet_on(k: &Kernel, pb: &ProceduralBlend, span: &DefinitionSpan) -> Result<bool> {
    for (f, support) in pb.supports().into_iter().enumerate() {
        if AnalyticSupport::of(support).is_some() {
            let finite = span.stations.iter().all(|st| {
                param(st, 2 * f, 0, 0).is_finite() && param(st, 2 * f + 1, 0, 0).is_finite()
            });
            if !finite {
                return Ok(false);
            }
            continue;
        }
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
