//! A proved bound on how far a curve lies from a face's surface, over the whole curve (not
//! upstream, tangency-labs/solidcraft#16508): what a construction that fits a surface to its
//! input curves records as its edges' tolerance, so the B-rep states its real construction
//! error (a sampled distance is a lower estimate of the largest one,
//! `docs/validity-and-tolerances.md` section 8).
//!
//! The curve `C(t)` is a B-spline, the surface `S` a NURBS net, and `W(t)` a B-spline of
//! parameters `(u, v, 0)` on that net, a correspondence chosen only to be close to the nearest
//! point (it is built through the nearest points of the curve's own samples). Whatever `W` is,
//! `dist(C(t), S) <= |g(t)|` for `g = C - S(W)`, because `S(W(t))` is a point of `S`. On a
//! piece `[a, b]` of the curve, `g` is within `h^2 / 8 * sup |g''|` of the chord between its two
//! end values (`h = b - a`), so
//!
//! `max over [a, b] of dist(C, S) <= max(|g(a)|, |g(b)|) + h^2 / 8 * sup |g''|`.
//!
//! The end values and `sup |g''|` are enclosed in outward-rounded interval arithmetic: the
//! curve's and `W`'s derivatives over the piece ([`curve_derivs_over`]), the surface's partials
//! over the box `W` sweeps there (`NetSurface::partials`, which cuts the box at the knot lines
//! it crosses, each part on its own polynomial piece, and refuses one crossed where the surface
//! is not continuous enough for the order asked). Because
//! `C` and `S(W)` nearly coincide, `g''` is small and the second term shrinks with the fourth
//! power of the piece, so the pieces are bisected until it no longer matters. A piece whose box
//! crosses a knot line the surface is only C^0 across (a patch corner the curve ends on) has no
//! second derivative to bound; there `g` is Lipschitz, and the piece is bounded by its larger
//! end value plus `h * sup |g'|` over the one-sided partials, bisected the same way.
//!
//! A surface that is not a NURBS net (or an analytic surface's exact net), a net whose
//! parameters are not the surface's, a box past the net's end where the surface does not
//! continue, and a piece the bisection cannot close all give `None`: the gap is then not
//! enclosed.

use crate::blend::fillet_section::certificate::{
    curve_derivs_over, knot_cuts, nurbs_partials_over_piece, partials_hull, EndContinuation,
    NurbsCurve,
};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{
    as_b_spline, knot, nurbs_partials, span, Nurbs, Partials,
};
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::surface::surface::Surface;
use crate::{Result, SolidError};

/// A direction's knot samples (parameter, spacing, span index) and its slack.
type KnotSamples = (Vec<(f64, f64, usize)>, f64);

const OP: &str = "query::curve_surface_gap";

/// The most pieces the bisection may make before the gap is called not enclosed.
const MAX_PIECES: usize = 1 << 15;
/// The deepest bisection of one piece.
const MAX_DEPTH: u32 = 30;
/// A piece is closed once its second-order term is at most this fraction of its end values.
const CLOSE_RELATIVE: f64 = 1.0e-3;
/// ... or at most this absolute amount, whatever the caller's floor: below any tolerance a
/// model carries.
const CLOSE_ABSOLUTE: f64 = 1.0e-13;
/// Points of the curve the correspondence `W` passes through (their nearest points on the
/// surface): only how close `W` stays to the nearest point, so how tight the bound is, depends
/// on it, never whether it holds.
const CORRESPONDENCE_POINTS: usize = 257;
/// Pieces the bisection starts from.
const FIRST_PIECES: usize = 32;

/// A surface prepared for enclosure: its net and the directions it continues past the net.
struct NetSurface {
    net: Nurbs,
    ends: EndContinuation,
}

impl NetSurface {
    /// The net of `surface`, or `None` when it has none (an offset, a step surface, a surface
    /// in a volume, a curve-bounded one) or its net's parameters are not the surface's: the
    /// net and the surface must give one point at the parameters `probe`.
    fn of(k: &Kernel, surface: &Surface, probe: &[Point2]) -> Result<Option<Self>> {
        let Some(bs) = as_b_spline(k, surface)? else {
            return Ok(None);
        };
        let Ok(net) = Nurbs::of(&bs) else {
            return Ok(None);
        };
        let ends = EndContinuation::of(surface);
        let ends = if ends.apex.is_some() {
            EndContinuation::default()
        } else {
            ends
        };
        let me = Self { net, ends };
        for &uv in probe {
            let Some(on_net) = me.point(uv) else {
                return Ok(None);
            };
            let flags = crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags {
                u_side: crate::nurbs::knots::Side::Left,
                v_side: crate::nurbs::knots::Side::Left,
                only_upper_half: true,
                non_zero_tangents: true,
            };
            let Some(on_surface) = crate::error::ignored(surface.evaluate(k, uv, 0, 0, flags))?
                .and_then(|d| d.into_iter().next().and_then(|row| row.into_iter().next()))
            else {
                return Ok(None);
            };
            let scale = 1.0 + on_surface.x.abs() + on_surface.y.abs() + on_surface.z.abs();
            if on_net.distance(on_surface) > 1.0e-9 * scale {
                return Ok(None);
            }
        }
        Ok(Some(me))
    }

    /// The net's point at `uv` (double arithmetic, a sanity probe only).
    fn point(&self, uv: Point2) -> Option<crate::math::Vec3> {
        let n = &self.net;
        let ku = span(&n.u, n.p, n.nu, uv.x);
        let kv = span(&n.v, n.q, n.nv, uv.y);
        let p = nurbs_partials(n, ku, kv, uv.x, uv.y, 0);
        let v = p.at(0, 0);
        let v = crate::math::Vec3::new(v.x, v.y, v.z);
        (v.x.is_finite() && v.y.is_finite() && v.z.is_finite()).then_some(v)
    }

    /// The surface's partials up to order `lk` over the box `w[0]` sweeps, with the slack the
    /// box's overshoot past the net needs (zero for a box inside it).
    ///
    /// The surface in the box is the net's polynomial pieces, the end span's polynomial
    /// continued past an end of the net: a surface that is a line in that direction continues
    /// (a plane, a cylinder's generator), and it is then the surface itself. Elsewhere only
    /// rounding overshoots a net's end (a seam on the face's boundary, its parameters a ulp
    /// out): the continued polynomial `S~` differs from the surface at the clamped point by at
    /// most the first partials' size times the overshoot, which is the slack, added to the
    /// bound: `dist(C, S) <= |C - S~(W)| + |S~(W) - S(clamp W)|`.
    fn partials(
        &self,
        w: &[V3<Ivl>],
        lk: usize,
        continuity: usize,
    ) -> Result<(Partials<Ivl>, f64)> {
        let refused = |reason| SolidError::Unsupported {
            operation: OP,
            reason,
        };
        let Some(w0) = w.first() else {
            return Err(refused("no parameter box"));
        };
        let n = &self.net;
        let dir = |knots: &[f64],
                   p: usize,
                   count: usize,
                   lo: f64,
                   hi: f64,
                   continues: bool|
         -> Result<KnotSamples> {
            let (d0, d1) = (knot(knots, p as isize), knot(knots, count as isize));
            if !(lo <= hi && lo.is_finite() && hi.is_finite()) {
                return Err(refused("an unordered parameter box"));
            }
            let slack_range = 0.05 * (d1 - d0).abs();
            let over = (d0 - lo).max(hi - d1).max(0.0);
            let continues = continues && p == 1;
            if over > 0.0 && !continues && over > slack_range {
                return Err(refused("a box past the net's end"));
            }
            let mut out = Vec::new();
            // (a part past an end runs on to the end, so the way back to the clamped point is
            // inside the box the first partials are enclosed over)
            if lo < d0 {
                out.push((lo, d0, span(knots, p, count, d0)));
            }
            let (a, b) = (lo.max(d0), hi.min(d1));
            if a <= b {
                out.extend(knot_cuts(knots, p, count, a, b, continuity)?);
            }
            if hi > d1 {
                out.push((d1, hi, span(knots, p, count, d1)));
            }
            Ok((out, if continues { 0.0 } else { over }))
        };
        let (us, over_u) = dir(&n.u, n.p, n.nu, w0.x.lo, w0.x.hi, self.ends.u)?;
        let (vs, over_v) = dir(&n.v, n.q, n.nv, w0.y.lo, w0.y.hi, self.ends.v)?;
        let mut out: Option<Partials<Ivl>> = None;
        for &(a0, a1, ku) in &us {
            for &(b0, b1, kv) in &vs {
                let p = nurbs_partials_over_piece(n, ku, kv, a0, a1, b0, b1, lk)?;
                out = Some(match out {
                    None => p,
                    Some(h) => partials_hull(h, &p, lk),
                });
            }
        }
        let Some(out) = out else {
            return Err(refused("no piece encloses the box"));
        };
        // (the point values of an end are of the continued polynomial itself; only a span's
        // bound needs the slack, and it has the first partials)
        let mut slack = 0.0_f64;
        if lk >= 1 && (over_u > 0.0 || over_v > 0.0) {
            slack = Self::slack(&out, over_u, over_v)?;
        }
        Ok((out, slack))
    }

    fn slack(p: &Partials<Ivl>, over_u: f64, over_v: f64) -> Result<f64> {
        let (Some(su), Some(sv)) = (norm_upper(p.at(1, 0)), norm_upper(p.at(0, 1))) else {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "the surface's first partials are not enclosed",
            });
        };
        Ok((Ivl::from(su) * Ivl::from(over_u) + Ivl::from(sv) * Ivl::from(over_v)).hi)
    }
}

/// `sqrt(x^2 + y^2 + z^2)` of an enclosure, an upper bound; `None` unless finite.
fn norm_upper(v: V3<Ivl>) -> Option<f64> {
    let sq = |a: Ivl| {
        let m = Ivl::from(a.mag());
        m * m
    };
    let s = sq(v.x) + sq(v.y) + sq(v.z);
    let hi = s.sqrt().hi;
    (v.x.is_finite() && v.y.is_finite() && v.z.is_finite() && hi.is_finite()).then_some(hi)
}

fn scaled(v: V3<Ivl>, a: Ivl) -> V3<Ivl> {
    V3 {
        x: v.x * a,
        y: v.y * a,
        z: v.z * a,
    }
}

fn plus(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn minus(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// The bound of one piece: `(end values, second-order term)`.
fn piece(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    s: &NetSurface,
    a: f64,
    b: f64,
) -> Result<Option<(f64, f64)>> {
    let mut ends = 0.0_f64;
    for t in [a, b] {
        let at = Ivl::from(t);
        let c = curve_derivs_over(cn, at, 0, None)?;
        let w = curve_derivs_over(wn, at, 0, None)?;
        let (Some(c0), Some(_)) = (c.first().copied(), w.first()) else {
            return Ok(None);
        };
        let surf = s.partials(&w, 0, 0)?.0.at(0, 0);
        let Some(g) = norm_upper(minus(c0, surf)) else {
            return Ok(None);
        };
        ends = ends.max(g);
    }
    let span = Ivl::new(a, b);
    let cd = curve_derivs_over(cn, span, 2, None)?;
    let wd = curve_derivs_over(wn, span, 2, None)?;
    let (Some(c1), Some(c2), Some(w1), Some(w2)) = (cd.get(1), cd.get(2), wd.get(1), wd.get(2))
    else {
        return Ok(None);
    };
    let (u1, v1) = (w1.x, w1.y);
    let (u2, v2) = (w2.x, w2.y);
    let h = Ivl::from(b) - Ivl::from(a);
    // The second order, where the surface is C^1 across every knot line the box crosses:
    // (S o W)'' = S_uu u'^2 + 2 S_uv u'v' + S_vv v'^2 + S_u u'' + S_v v''
    let second = s.partials(&wd, 2, 1).map(|(p, slack)| {
        let mut swpp = scaled(p.at(2, 0), u1 * u1);
        swpp = plus(swpp, scaled(p.at(1, 1), Ivl::from(2.0) * u1 * v1));
        swpp = plus(swpp, scaled(p.at(0, 2), v1 * v1));
        swpp = plus(swpp, scaled(p.at(1, 0), u2));
        swpp = plus(swpp, scaled(p.at(0, 1), v2));
        (minus(*c2, swpp), slack)
    });
    let term = match second {
        Ok((gpp, slack)) => {
            let Some(m) = norm_upper(gpp) else {
                return Ok(None);
            };
            (h * h * Ivl::from(m) * Ivl::from(0.125) + Ivl::from(slack)).hi
        }
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        // a knot line the surface is only C^0 across (a patch corner the curve ends on):
        // g is Lipschitz there, |g(t) - g(a)| <= h sup |g'| with the one-sided partials hulled
        Err(_) => {
            let (p, slack) = s.partials(&wd, 1, 0)?;
            let sw = plus(scaled(p.at(1, 0), u1), scaled(p.at(0, 1), v1));
            let Some(m) = norm_upper(minus(*c1, sw)) else {
                return Ok(None);
            };
            (h * Ivl::from(m) + Ivl::from(slack)).hi
        }
    };
    Ok(term.is_finite().then_some((ends, term)))
}

/// A proved upper bound on the distance from the curve `curve` to the surface `surface`
/// over its parameters `[lo, hi]` (within its domain), where `uv` is a B-spline of parameters `(u, v, 0)` (the correspondence
/// `W`, on the curve's own parameter). A piece closes once its second term is at most a
/// thousandth of its end values or `floor` (the caller's scale: a hundredth of the gap it
/// expects, below which an excess in the bound changes nothing; a nearest point is found only
/// to some 1e-8 of a parameter, which the bound carries as well). A piece that cannot be cut
/// further is bounded as it stands, so the result is a bound however the pieces ended; `None`
/// when some piece has no bound at all.
fn certified_gap(
    curve: &BSplineCurve,
    uv: &BSplineCurve,
    surface: &NetSurface,
    [lo, hi]: [f64; 2],
    first_pieces: usize,
    floor: f64,
) -> Result<Option<f64>> {
    let (Ok(cn), Ok(wn)) = (NurbsCurve::of(curve), NurbsCurve::of(uv)) else {
        return Ok(None);
    };
    let (Some(&d0), Some(&d1)) = (cn.u.get(cn.p), cn.u.get(cn.n)) else {
        return Ok(None);
    };
    if !(lo.is_finite() && hi.is_finite() && d0 <= lo && lo < hi && hi <= d1) || first_pieces == 0 {
        return Ok(None);
    }
    let mut stack: Vec<(f64, f64, u32)> = (0..first_pieces)
        .rev()
        .map(|i| {
            let f = |j: usize| {
                if j == first_pieces {
                    hi
                } else {
                    lo + (hi - lo) * (j as f64) / (first_pieces as f64)
                }
            };
            (f(i), f(i + 1), 0)
        })
        .collect();
    let mut worst = 0.0_f64;
    let mut made = stack.len();
    while let Some((a, b, depth)) = stack.pop() {
        let mid = 0.5 * (a + b);
        // (a piece that cannot be cut further is bounded as it is: the bound holds whether or
        // not the second term has shrunk, closing only makes it tight)
        let last = depth >= MAX_DEPTH || !(a < mid && mid < b) || made + 2 > MAX_PIECES;
        match piece(&cn, &wn, surface, a, b) {
            Ok(Some((ends, term))) => {
                let bound = (Ivl::from(ends) + Ivl::from(term)).hi;
                if last || term <= (CLOSE_RELATIVE * ends).max(floor).max(CLOSE_ABSOLUTE) {
                    worst = worst.max(bound);
                    continue;
                }
            }
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            // a box past the net, on a knot line of too low a continuity: a smaller piece may
            // clear it; at the bottom the gap is not enclosed
            Ok(None) | Err(_) => {
                if last {
                    return Ok(None);
                }
            }
        }
        made += 2;
        stack.push((mid, b, depth + 1));
        stack.push((a, mid, depth + 1));
    }
    Ok(worst.is_finite().then_some(worst))
}

/// A proved upper bound on the distance from every point of `curve` over its parameters
/// `range` to the surface of face `face` over that surface's parameter box: no point of the
/// curve there is farther from the surface. `floor` is the caller's scale (a hundredth of the
/// tolerance the bound is held to, below which a tighter bound changes nothing).
///
/// `None` when the gap is not enclosed: a face without a surface or whose surface is not a
/// NURBS net ([`NetSurface::of`]), a curve point the surface cannot be dropped from, an empty
/// or unordered `range` or one past the curve's domain, or a piece of the curve the bisection
/// cannot bound.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling face; the kernel's unported and cancelled statuses.
pub fn curve_face_gap_bound(
    k: &Kernel,
    brep: &crate::topology::Brep,
    face: crate::topology::FaceId,
    curve: &BSplineCurve,
    range: [f64; 2],
    floor: f64,
) -> Result<Option<f64>> {
    use crate::curve::curve::Curve;
    use crate::error::ignored;
    use crate::nurbs::crv_fit::p1::ParamMethod;
    use crate::surface::surface::p4::NoObject;
    use crate::surface::surface::p5::DropPointOptions;

    let [lo, hi] = range;
    if !(lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
    let rec = brep.face(face)?;
    let Some(surface) = rec.surface.as_ref() else {
        return Ok(None);
    };
    let surface = surface.clone_for_evaluation()?;
    let domain = rec.uv_domain;
    let evaluated = Curve::from(curve.clone_exact());
    let params: Vec<f64> = (0..CORRESPONDENCE_POINTS)
        .map(|i| {
            if i + 1 == CORRESPONDENCE_POINTS {
                hi
            } else {
                lo + (hi - lo) * (i as f64) / ((CORRESPONDENCE_POINTS - 1) as f64)
            }
        })
        .collect();
    // The nearest points are followed from the middle of the curve out to each end, each drop
    // started from its neighbour's: an end on a side of the net collapsed to one point (the apex
    // of a loft between curves that meet there) has every parameter of that side for its nearest
    // point, and a drop with nothing to start from takes any of them, so `W` would jump there
    // and run past the net. Continued from its neighbour it takes the neighbour's; the bound holds
    // for any `W`, only whether the bisection closes depends on this.
    let n = params.len();
    let mid = n / 2;
    let order: Vec<(usize, Option<usize>)> = (mid..n)
        .map(|i| (i, (i > mid).then(|| i - 1)))
        .chain((0..mid).rev().map(|i| (i, Some(i + 1))))
        .collect();
    let mut found: Vec<Option<Point2>> = vec![None; n];
    for (i, from) in order {
        let Some(&t) = params.get(i) else {
            return Ok(None);
        };
        let guess = from.and_then(|j| found.get(j).copied().flatten());
        let Some(point) =
            ignored(evaluated.evaluate(k, t, 0, true, false))?.and_then(|d| d.into_iter().next())
        else {
            return Ok(None);
        };
        let opts = DropPointOptions {
            guess_uv: guess,
            ..DropPointOptions::default()
        };
        let Some(drop) = ignored(surface.drop_point::<NoObject>(k, point, &domain, &opts))? else {
            return Ok(None);
        };
        if !drop.success {
            return Ok(None);
        }
        if let Some(slot) = found.get_mut(i) {
            *slot = Some(drop.result_uv);
        }
    }
    let uvs: Vec<Point2> = found.into_iter().flatten().collect();
    let Some(net) = NetSurface::of(k, &surface, &uvs)? else {
        return Ok(None);
    };
    let flat: Vec<crate::math::Vec3> = uvs
        .iter()
        .map(|uv| crate::math::Vec3::new(uv.x, uv.y, 0.0))
        .collect();
    let Ok(correspondence) = BSplineCurve::interpolate_points(
        &flat,
        Some(&params),
        3,
        None,
        None,
        false,
        ParamMethod::ChordLength,
    ) else {
        return Ok(None);
    };
    certified_gap(curve, &correspondence, &net, range, FIRST_PIECES, floor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Point3;
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW};

    /// A line `lift` above a box's top face, inside it: its gap to the top face's plane is
    /// `lift` everywhere, so the bound is at least `lift` and, the line and the plane being
    /// polynomial, no more than rounding above it; the faces it is far from bound it far.
    #[test]
    fn a_line_above_a_face_is_bounded_by_its_height() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
            .unwrap();
        let lift = 6.8e-5;
        let z = 3.0 + lift;
        let line = NurbCurve::new(
            vec![CPoint::new(0.2, 0.3, z, NOW), CPoint::new(0.7, 1.6, z, NOW)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let line = BSplineCurve::from_nurb(line, 3, false);
        let mut bounds = Vec::new();
        for f in crate::query::api_queries::get_faces(&b).unwrap() {
            if let Some(g) = curve_face_gap_bound(&k, &b, f, &line, [0.0, 1.0], 1e-7).unwrap() {
                bounds.push(g);
            }
        }
        let nearest = bounds.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            nearest >= lift && nearest <= lift * (1.0 + 1e-6),
            "{nearest:e} against {lift:e}: {bounds:?}"
        );
        // an empty or reversed range encloses nothing
        let top = crate::query::api_queries::get_faces(&b).unwrap();
        assert!(top.iter().all(
            |f| curve_face_gap_bound(&k, &b, *f, &line, [0.5, 0.5], 1e-7)
                .unwrap()
                .is_none()
        ));
    }
}
