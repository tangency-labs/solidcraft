//! The coincident region of a surface of extrusion lying in a plane: when the
//! extrusion vector is perpendicular to the plane normal and a range of the generator
//! lies in the plane, the surfaces coincide over a 2D region `R`. The boundary of `R`,
//! typed `RegionBoundary`, is the intersection's answer, as for the extrusion /
//! extrusion and revolution special cases.
//!
//! Upstream: none (not upstream: `SmSurfOfExtrusion::IntersectWithPlane` asks for the
//! general intersection here, whose answer then depends on its tolerance).
//!
//! | Rust                       | Upstream |
//! |----------------------------|----------|
//! | `coincident_patch` (crate) | none     |
//!
//! Deviations: not a port. In the plane's UV coordinates write `mu` for the
//! coordinate across the sweep and `nu` for the extrusion parameter along it: the
//! surface point `C(s) + v E` has `mu = mu(C(s))` and `nu = nu(C(s)) + v`. The image
//! `I` of the generator ranges swept over `V = [vmin, vmax]` is cut to the plane's
//! domain box `D`: `R = I ∩ D`. Two configurations are answered, each on a certificate;
//! any other declines (the caller keeps the general intersection, as upstream):
//! - **monotone**: one range whose `mu` is monotone (certified on its control
//!   polygon, see `range_piece`), each flat stretch (along the sweep) spanning less
//!   than the sweep so that its two copies bound it without overlapping. Then every
//!   `mu` but those of flat stretches meets the range once, the slice of `I` at `mu`
//!   is one interval `[nu(s) + vmin, nu(s) + vmax]`, and the slice of `R` is that
//!   interval cut to the slice of `D`. The boundary is swept along the range: below,
//!   the copy at `vmin` or the side of `D` above it; above, the copy at `vmax` or the
//!   side of `D` below it; across, the directrices at the range ends and the sides of
//!   `D` parallel to the sweep, each cut to `D`. A copy and a side that coincide give
//!   the side only, and a side parallel to the sweep within the slack of an end's
//!   `mu` stands for that end's directrix.
//! - **slab**: ranges whose `mu` ranges overlap (one connected `mu` interval
//!   `[alpha, beta]`, folds allowed), when every `nu` of `D` in the slab
//!   `alpha <= mu <= beta` is within `[nu_max + vmin, nu_min + vmax]` (`nu_min`,
//!   `nu_max` bound `nu` over the ranges' control points). Every slice of `R` is then
//!   the slice of `D`, so `R` is the convex polygon `D` cut to the slab, and its edges
//!   are the answer: the fold (or end) directrices at `alpha` and `beta` and the parts
//!   of `D`'s sides between them.
//!
//! The ranges are the solver's, checked and completed here. Every piece of the
//! generator a certificate reads is split from it exactly at its ends (no zone
//! tolerance, so a short end span is kept) and certified on its control net (see
//! `net_points`). A range end inside the generator interval is followed span by span
//! while the next span is certified in the plane (the range extends over it, merging
//! ranges it reaches, and absorbs the point solutions inside); then the span past it
//! must be certified to leave the plane (`past_end`): beyond a parameter whose
//! generator point is within the tolerance of the end, it is farther from the plane
//! than the in-plane bound, and a point solution where it may still be within the
//! solver's tolerance of the plane declines (it is that same departure).
//!
//! The monotone sweep's breaks (where `mu` passes a corner of `D`, where a copy
//! crosses a side) are isolated on control nets (`roots`: a piece whose values keep
//! one certain sign has no root, one with a single certain sign change has exactly
//! one, found by bisection; others are halved), and the slab's `alpha`, `beta` are a
//! branch and bound over control nets (`extreme`): within `extreme_width` of the true
//! extremes, at an attained value. Both searches share a budget of `PIECE_BUDGET`
//! split pieces and decline past it.
//!
//! A point solution outside the ranges whose directrix (a line of constant `mu`)
//! meets a region's `[alpha, beta]` declines: it would cross the region or repeat
//! its edge.
//!
//! Ranges with disjoint `mu` ranges have disjoint regions, answered one by one. A
//! region of no area (a range of constant `mu`, a slab touching `D` at a point)
//! declines. Every loop is bounded by the number of ranges, knots, domain sides and
//! the piece budget; nothing recurses (the searches use explicit worklists).
//!
//! Every slack is relative to the magnitudes it compares (no absolute floor), so the
//! certificates hold at every model scale.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignore_status, Result};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{EFF_ZERO, EFF_ZERO_PARAM};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::math::gamma;
use crate::surface::plane::Plane;
use crate::surface::surface::p2::TsectCurveType;

use super::p1::gen_base;

/// The coincident patch's inputs: the generator, the extrusion vector and interval,
/// the plane and its domain, and the tolerance of the special case.
pub(super) struct PatchInputs<'a> {
    /// The generator (with the surface's out-of-bounds flag).
    pub gen: &'a Curve,
    /// The extrusion vector `E`: the surface is `C(s) + v E`.
    pub extrusion_vec: Vec3,
    /// The generator interval of the caller's domain.
    pub gen_ivl: Extent1d,
    /// Whether the generator is closed over `gen_ivl` (a range end at one end of the
    /// interval continues at the other).
    pub gen_closed: bool,
    /// The extrusion interval `V` of the caller's domain.
    pub v_ivl: Extent1d,
    /// The plane.
    pub plane: &'a Plane,
    /// The plane's domain `D`.
    pub plane_domain: Extent2d,
    /// The special case's tolerance (`min(approx, SM_EFF_ZERO_SQRT)`).
    pub tol: f64,
}

/// The coincident patch: its boundary curves, and the generator ranges it covers
/// (the solver's, extended and merged; the point solutions inside them are absorbed).
pub(super) struct Patch {
    /// The boundary curves, typed `RegionBoundary`.
    pub curves: Vec<(Curve, TsectCurveType)>,
    /// The generator ranges covered.
    pub ranges: Vec<(f64, f64)>,
}

/// `|x| + |y| + |z|`.
fn norm1(v: Vec3) -> f64 {
    v.x.abs() + v.y.abs() + v.z.abs()
}

/// The relative slack of the comparisons that decide ties and the slab's cover:
/// far above the rounding of the coordinates compared (a few `gamma(8)`), far below
/// any geometric tolerance. Within it a copy and a side of the domain coincide.
const REL_SLACK: f64 = 1e-12;

/// The most generator pieces the certified searches of one patch may split (each a
/// trim and a control net); past it the patch declines.
const PIECE_BUDGET: usize = 4000;

/// The narrowest piece (relative to its range) the root isolation halves; a piece
/// still undecided there gives a break at its middle (a break is harmless: it only
/// splits a run of the sweep).
const MIN_SPLIT: f64 = 1.0 / 68_719_476_736.0; // 2^-36

/// A generator piece's control net: its Euclidean control points with a bound on
/// each one's error from the exact split of the generator, its weights (1 for a
/// polynomial piece), the relative error bound of those weights, and its degree.
struct Net {
    pts: Vec<(Point3, f64)>,
    weights: Vec<f64>,
    weight_err: f64,
    degree: usize,
}

impl Net {
    /// The largest coordinate magnitude of the control points.
    fn scale(&self) -> f64 {
        self.pts
            .iter()
            .fold(0.0_f64, |m, &(p, _)| m.max(p.max_dimension()))
    }
}

/// The control net of a generator piece: the split's rounding (at most `2p` knot
/// insertions of three operations each, on homogeneous coordinates) and the division
/// by the weight bound each point's error. `None` for non-finite data or a
/// non-positive weight (no NaN passes the comparisons).
fn net_points(piece: &Curve) -> Option<Net> {
    let base = gen_base(piece).ok()?;
    let canon = base.canonical().ok()?;
    let weights_of = |i: usize| -> f64 {
        if canon.weights.is_empty() {
            1.0
        } else {
            canon.weights.get(i).copied().unwrap_or(f64::NAN)
        }
    };
    let mut max_hom = 0.0_f64;
    let mut max_w = 0.0_f64;
    for (i, p) in canon.control_points.iter().enumerate() {
        let w = weights_of(i);
        let valid = w > 0.0 && w.is_finite();
        if !valid {
            return None;
        }
        max_hom = max_hom.max(w * norm1(*p));
        max_w = max_w.max(w);
    }
    let finite = max_hom.is_finite();
    if canon.control_points.is_empty() || !finite {
        return None;
    }
    let split_ops = 6 * canon.degree + 4;
    let mut pts = Vec::new();
    let mut weights = Vec::new();
    for (i, &p) in canon.control_points.iter().enumerate() {
        let w = weights_of(i);
        pts.push((p, gamma(split_ops) * (max_hom + norm1(p) * max_w) / w));
        weights.push(w);
    }
    let weight_err = if canon.weights.is_empty() {
        0.0
    } else {
        gamma(split_ops)
    };
    Some(Net {
        pts,
        weights,
        weight_err,
        degree: canon.degree,
    })
}

/// The generator piece over `[lo, hi]`, split exactly at its ends (zone tolerance 0:
/// no short end span is dropped, so a certificate on the piece covers the generator
/// itself), with the interval made (snapped to a knot within the split's rounding).
/// `None` when the split fails (the caller declines).
fn split(k: &Kernel, gen: &Curve, lo: f64, hi: f64) -> Option<(Curve, Extent1d)> {
    let ordered = lo < hi;
    if !ordered {
        return None;
    }
    let mut ivl = Extent1d::from_min_max(lo, hi);
    let mut piece = gen.clone();
    let made = ignore_status(piece.trim_with_zone_tol(k, &mut ivl, TrimMode::Silent, 0.0));
    let ok = made.is_ok() && ivl.min() < ivl.max();
    ok.then_some((piece, ivl))
}

/// A bound on the error of `d . (P - O)` computed for a control point `P` known to
/// `err`: the point's error and the rounding of the difference and the dot product.
fn functional_err(d: Vec3, p: Point3, o: Point3, err: f64) -> f64 {
    norm1(d) * (err + gamma(8) * (norm1(p) + norm1(o)))
}

/// The affine function `g(X) = d . (X - o) - c` of the plane's points.
#[derive(Clone, Copy)]
struct Affine {
    d: Vec3,
    o: Point3,
    c: f64,
}

impl Affine {
    /// `g` at a control point known to `err`, with a bound on the value's error.
    fn at(&self, p: Point3, err: f64) -> (f64, f64) {
        let x = self.d.dot(p - self.o);
        (
            x - self.c,
            functional_err(self.d, p, self.o, err) + gamma(1) * (x.abs() + self.c.abs()),
        )
    }

    /// `g` at a point.
    fn value(&self, p: Point3) -> f64 {
        self.d.dot(p - self.o) - self.c
    }
}

/// The certificate that the patch over the generator piece `net` lies in the plane,
/// in two parts. The sweep: `|n . E| + err <= SM_EFF_ZERO |E|` (`E` in the plane to
/// the working precision). The profile: every Euclidean control point `P` has
/// `|n . (P - O)| + err <= SM_EFF_ZERO s`, `s` the largest coordinate of the net (the
/// scale of the piece's coordinates, so the bound is relative at every model scale).
/// With positive weights a rational curve lies in the convex hull of its Euclidean
/// control points, so every point `C(s)` of the piece is within that bound of the
/// plane, and every patch point `C(s) + v E` within that plus `|v| SM_EFF_ZERO |E|`:
/// the scale of the point itself. `err` counts the rounding of the split, of `P - O`,
/// of the dot products and of the plane normal (`|n|_1`).
fn certify_in_plane(net: &Net, e: Vec3, plane: &Plane) -> bool {
    let n = plane.position().z_axis();
    let o = plane.position().origin();
    let n1 = norm1(n);
    let err_e = n1 * gamma(4) * norm1(e);
    let sweep_in_plane = n.dot(e).abs() + err_e <= EFF_ZERO * e.max_dimension();
    if !sweep_in_plane {
        return false;
    }
    let bound = EFF_ZERO * net.scale();
    net.pts.iter().all(|&(p, err)| {
        // (a NaN anywhere fails the check)
        n.dot(p - o).abs() + functional_err(n, p, o, err) <= bound
    })
}

/// Bisects for the single root of `g(C(s))` in `[a, b]`, `g` negative at `a` when
/// `neg_at_a` (on evaluations: the root to the rounding of `g`).
fn bisect(
    k: &Kernel,
    gen: &Curve,
    g: Affine,
    (mut a, mut b): (f64, f64),
    neg_at_a: bool,
) -> Result<f64> {
    // (a halving per bit of the parameter: at most a few hundred)
    for _ in 0..1100 {
        let m = a + (b - a) / 2.0;
        let interior = a < m && m < b;
        if !interior {
            break;
        }
        let v = g.value(gen.evaluate_point(k, m)?);
        if v == 0.0 {
            return Ok(m);
        }
        if (v < 0.0) == neg_at_a {
            a = m;
        } else {
            b = m;
        }
    }
    Ok(a + (b - a) / 2.0)
}

/// The parameters of `ivl` where `g(C(s))` may vanish, certified on control nets
/// (with positive weights the number of roots of a piece is at most the sign changes
/// of its control values: variation diminishing): a piece whose values all keep one
/// certain sign has none; one whose values have certain signs with one change has
/// exactly one (its end values are its end points'), found by bisection; one whose
/// values are all within their errors of zero may vanish throughout and gives its
/// ends; any other is halved, down to `MIN_SPLIT` of `ivl`, where it gives its middle.
/// Every root lies in a returned parameter's piece. `None` past the budget or when a
/// split fails.
fn roots(
    k: &Kernel,
    gen: &Curve,
    ivl: &Extent1d,
    g: Affine,
    budget: &mut usize,
) -> Result<Option<Vec<f64>>> {
    let min_width = (ivl.max() - ivl.min()) * MIN_SPLIT;
    let mut todo = vec![(ivl.min(), ivl.max())];
    let mut out = Vec::new();
    while let Some((a, b)) = todo.pop() {
        if *budget == 0 {
            return Ok(None);
        }
        *budget -= 1;
        let Some((piece, _)) = split(k, gen, a, b) else {
            return Ok(None);
        };
        let Some(net) = net_points(&piece) else {
            return Ok(None);
        };
        let vals: Vec<(f64, f64)> = net.pts.iter().map(|&(p, err)| g.at(p, err)).collect();
        let positive = vals.iter().all(|&(v, err)| v > err);
        let negative = vals.iter().all(|&(v, err)| v < -err);
        if positive || negative {
            continue;
        }
        let certain = vals.iter().all(|&(v, err)| v.abs() > err);
        let changes = vals
            .windows(2)
            .filter(|w| matches!(w, [(x, _), (y, _)] if (*x < 0.0) != (*y < 0.0)))
            .count();
        if certain && changes == 1 {
            let neg_at_a = vals.first().is_some_and(|&(v, _)| v < 0.0);
            out.push(bisect(k, gen, g, (a, b), neg_at_a)?);
            continue;
        }
        let on = vals.iter().all(|&(v, err)| v.abs() <= err);
        if on {
            out.extend([a, b]);
            continue;
        }
        let m = a + (b - a) / 2.0;
        let splits = b - a > min_width && a < m && m < b;
        if !splits {
            out.push(m);
            continue;
        }
        todo.push((m, b));
        todo.push((a, m));
    }
    Ok(Some(out))
}

/// The width within which `extreme` brackets an extreme of `mu` over a range whose
/// control net has `mus` (values and errors): the relative slack of the values and
/// twice their largest error (a piece's hull bound is within those of its values).
fn extreme_width(mus: &[(f64, f64)]) -> f64 {
    let (scale, err) = mus.iter().fold((0.0_f64, 0.0_f64), |(s, e), &(m, me)| {
        (s.max(m.abs()), e.max(me))
    });
    REL_SLACK * scale + 2.0 * err
}

/// The least value of `sign * mu(C(s))` over `ivl` (`sign` 1 for the least `mu`, -1
/// for the greatest), by branch and bound on control nets: a piece's least control
/// value (less its error) bounds it below (convex hull), the evaluated values at the
/// piece ends bound the extreme above, and the least-bounded piece is halved until
/// every piece's bound is within `width` of the best value found. The value returned
/// is attained (an evaluation), within `width` (and the rounding of the evaluations)
/// of the true extreme. The initial pieces are the knot spans, so a corner of the
/// generator is an evaluated end. `None` past the budget or when a split fails.
fn extreme(
    k: &Kernel,
    inputs: &PatchInputs<'_>,
    frame: &Frame,
    ivl: &Extent1d,
    sign: f64,
    width: f64,
    budget: &mut usize,
) -> Result<Option<f64>> {
    let gen = inputs.gen;
    let mu_at = |s: f64| -> Result<f64> {
        Ok(sign
            * frame.mu(inputs
                .plane
                .project_point_to_uv_domain(gen.evaluate_point(k, s)?)))
    };
    let g = Affine {
        d: frame.dmu * sign,
        o: frame.o,
        c: 0.0,
    };
    let mut lower_of = |a: f64, b: f64| -> Option<f64> {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        let (piece, _) = split(k, gen, a, b)?;
        let net = net_points(&piece)?;
        let lb = net
            .pts
            .iter()
            .map(|&(p, err)| {
                let (v, e) = g.at(p, err);
                v - e
            })
            .fold(f64::INFINITY, f64::min);
        lb.is_finite().then_some(lb)
    };
    let mut cuts = vec![ivl.min()];
    if let CurveKind::BSpline(bs) = gen.kind() {
        cuts.extend(
            bs.knots(None)?
                .0
                .into_iter()
                .filter(|&t| t > ivl.min() && t < ivl.max()),
        );
    }
    cuts.push(ivl.max());
    let mut upper = f64::INFINITY;
    for &t in &cuts {
        upper = upper.min(mu_at(t)?);
    }
    let mut cands: Vec<(f64, f64, f64)> = Vec::new();
    for w in cuts.windows(2) {
        let [a, b] = w else {
            continue;
        };
        let Some(lb) = lower_of(*a, *b) else {
            return Ok(None);
        };
        cands.push((lb, *a, *b));
    }
    loop {
        // the least-bounded piece (the first of equals)
        let mut best: Option<(usize, f64)> = None;
        for (i, &(lb, _, _)) in cands.iter().enumerate() {
            if best.is_none_or(|(_, b)| lb < b) {
                best = Some((i, lb));
            }
        }
        let Some((i, lb)) = best else {
            break;
        };
        let bracketed = lb >= upper - width;
        if bracketed {
            break;
        }
        // take candidate `i`, moving the last into its place (`swap_remove`'s order)
        let Some(last) = cands.pop() else {
            break;
        };
        let (_, a, b) = match cands.get_mut(i) {
            Some(slot) => std::mem::replace(slot, last),
            None => last,
        };
        let m = a + (b - a) / 2.0;
        let interior = a < m && m < b;
        if !interior {
            return Ok(None);
        }
        upper = upper.min(mu_at(m)?);
        for (x, y) in [(a, m), (m, b)] {
            let Some(lb) = lower_of(x, y) else {
                return Ok(None);
            };
            if lb < upper - width {
                cands.push((lb, x, y));
            }
        }
    }
    Ok(upper.is_finite().then_some(upper))
}

/// The plane's UV frame of the sweep: `E` in UV (`(eu, ew)`, a component within its
/// rounding of zero taken as zero, so a side of the domain parallel to the sweep to
/// the working precision is parallel), and the 3D functionals of the UV coordinates
/// and of `mu = -ew u + eu w` (across the sweep).
struct Frame {
    eu: f64,
    ew: f64,
    o: Point3,
    du: Vec3,
    dw: Vec3,
    dmu: Vec3,
    dnu: Vec3,
}

impl Frame {
    /// The frame, or `None` when the sweep has no UV component.
    fn new(plane: &Plane, e: Vec3) -> Option<Frame> {
        let position = *plane.position();
        let scale = plane.uv_scale();
        let du = position.x_axis() / scale.x;
        let dw = position.y_axis() / scale.y;
        // (a dot product of three terms and a division: within gamma(4) relative)
        let snap = |d: Vec3| {
            let x = d.dot(e);
            if x.abs() <= gamma(4) * norm1(d) * norm1(e) {
                0.0
            } else {
                x
            }
        };
        let (eu, ew) = (snap(du), snap(dw));
        let e2 = eu * eu + ew * ew;
        let usable = e2 > 0.0 && e2.is_finite();
        if !usable {
            return None;
        }
        Some(Frame {
            eu,
            ew,
            o: position.origin(),
            du,
            dw,
            dmu: dw * eu - du * ew,
            dnu: (du * eu + dw * ew) / e2,
        })
    }

    /// `mu` of a UV point.
    fn mu(&self, uv: Point2) -> f64 {
        -self.ew * uv.x + self.eu * uv.y
    }

    /// `nu` of a UV point (the extrusion parameter along the sweep).
    fn nu(&self, uv: Point2) -> f64 {
        (self.eu * uv.x + self.ew * uv.y) / (self.eu * self.eu + self.ew * self.ew)
    }

    /// The UV component along a domain axis (0 = u, 1 = w).
    fn e_along(&self, axis: usize) -> f64 {
        if axis == 0 {
            self.eu
        } else {
            self.ew
        }
    }
}

/// A side of the domain box: its fixed axis (0 = u, 1 = w) and value.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Side {
    axis: usize,
    value: f64,
}

/// The four sides of the domain box, in the order `u min, u max, w min, w max`.
fn sides_of(dom: &Extent2d) -> [Side; 4] {
    [
        Side {
            axis: 0,
            value: dom.min().x,
        },
        Side {
            axis: 0,
            value: dom.max().x,
        },
        Side {
            axis: 1,
            value: dom.min().y,
        },
        Side {
            axis: 1,
            value: dom.max().y,
        },
    ]
}

/// The point of the (non-parallel) side `side` at `mu`, its running coordinate
/// clamped to the box (the side's fixed coordinate exact).
fn side_point(frame: &Frame, dom: &Extent2d, side: Side, mu: f64) -> Point2 {
    if side.axis == 0 {
        // mu = -ew u + eu w
        let w = ((mu + frame.ew * side.value) / frame.eu)
            .max(dom.min().y)
            .min(dom.max().y);
        Point2::new(side.value, w)
    } else {
        let u = ((frame.eu * side.value - mu) / frame.ew)
            .max(dom.min().x)
            .min(dom.max().x);
        Point2::new(u, side.value)
    }
}

/// The extrusion parameters `v` where the line `uv + v (eu, ew)` lies in the domain
/// box, with the side that bounds each end (`None` for an end bounded by no side),
/// or `None` when the line misses the box.
fn chord(
    frame: &Frame,
    dom: &Extent2d,
    uv: Point2,
) -> Option<(f64, Option<Side>, f64, Option<Side>)> {
    let sides = sides_of(dom);
    let mut lo = (f64::NEG_INFINITY, None);
    let mut hi = (f64::INFINITY, None);
    for axis in 0..2 {
        let (p, d) = if axis == 0 {
            (uv.x, frame.eu)
        } else {
            (uv.y, frame.ew)
        };
        let (s_min, s_max) = if axis == 0 {
            (sides.first()?, sides.get(1)?)
        } else {
            (sides.get(2)?, sides.get(3)?)
        };
        if d == 0.0 {
            let inside = s_min.value <= p && p <= s_max.value;
            if !inside {
                return None;
            }
            continue;
        }
        let (t_min, t_max) = ((s_min.value - p) / d, (s_max.value - p) / d);
        let ((enter, enter_side), (leave, leave_side)) = if d > 0.0 {
            ((t_min, *s_min), (t_max, *s_max))
        } else {
            ((t_max, *s_max), (t_min, *s_min))
        };
        if enter > lo.0 {
            lo = (enter, Some(enter_side));
        }
        if leave < hi.0 {
            hi = (leave, Some(leave_side));
        }
    }
    let meets = lo.0 <= hi.0;
    meets.then_some((lo.0, lo.1, hi.0, hi.1))
}

/// The directrix through the generator point `p`, swept over `V` and cut to the
/// domain: the line `uv + v (eu, ew)` from `p`'s UV point (`v` measured from `p`, so
/// its precision does not depend on `|V| |E|`), over `V` cut to its chord of the box.
/// An end cut by a side is that side's point at the directrix's `mu` (`side_point`:
/// the side's coordinate exactly, the same point as the side segments' ends at that
/// `mu`); an uncut end is `p + v E` (the copies' end there). `on_side`: the directrix
/// lies along that side parallel to the sweep, whose coordinate it keeps exactly.
/// `None` outside the domain or of no length.
fn directrix(
    inputs: &PatchInputs<'_>,
    frame: &Frame,
    p: Point3,
    on_side: Option<Side>,
) -> Result<Option<Curve>> {
    let plane = inputs.plane;
    let dom = inputs.plane_domain;
    let e = inputs.extrusion_vec;
    let (vmin, vmax) = (inputs.v_ivl.min(), inputs.v_ivl.max());
    let mut uv = plane.project_point_to_uv_domain(p);
    let fix = |q: &mut Point2| match on_side {
        Some(Side { axis: 0, value }) => q.x = value,
        Some(Side { value, .. }) => q.y = value,
        None => {}
    };
    fix(&mut uv);
    let finite = uv.x.is_finite() && uv.y.is_finite();
    if !finite {
        return Ok(None);
    }
    let Some((dlo, lo_side, dhi, hi_side)) = chord(frame, &dom, uv) else {
        return Ok(None);
    };
    let mu = frame.mu(uv);
    let end = |v: f64, cut: Option<Side>| -> Point3 {
        match (cut, on_side) {
            (Some(side), _) => {
                let mut q = side_point(frame, &dom, side, mu);
                fix(&mut q);
                plane.evaluate_point_fast(q)
            }
            (None, Some(_)) => {
                let mut q = Point2::new(uv.x + v * frame.eu, uv.y + v * frame.ew);
                fix(&mut q);
                plane.evaluate_point_fast(q)
            }
            (None, None) => p + e * v,
        }
    };
    let (v0, cut0) = if dlo > vmin {
        (dlo, lo_side)
    } else {
        (vmin, None)
    };
    let (v1, cut1) = if dhi < vmax {
        (dhi, hi_side)
    } else {
        (vmax, None)
    };
    let spans = v0 < v1;
    if !spans {
        return Ok(None);
    }
    segment(end(v0, cut0), end(v1, cut1))
}

/// The segment `a`-`b`, or `None` when it has no length (within `SM_EFF_ZERO`
/// relative to its coordinates).
fn segment(a: Point3, b: Point3) -> Result<Option<Curve>> {
    let short = a.distance(b) <= EFF_ZERO * a.max_dimension().max(b.max_dimension());
    if short {
        return Ok(None);
    }
    Ok(Some(Curve::from(BSplineCurve::create_line_segment(
        3, a, b,
    )?)))
}

/// A generator range, split from the generator and certified in the plane, with its
/// `mu` extent (its least and greatest `mu`), whether its `mu` is certified
/// monotone, and bounds on its `nu`.
struct RangePiece {
    ivl: Extent1d,
    alpha: f64,
    beta: f64,
    monotone: bool,
    nu_lo: f64,
    nu_hi: f64,
}

/// A generator range measured by [`range_piece`].
enum Measured {
    /// The range fails a certificate: the patch declines.
    Declined,
    /// The range is empty in the caller's interval.
    Empty,
    /// The range, certified in the plane.
    Piece(RangePiece),
}

/// Whether every maximal run of flat control-polygon steps (`mu` steps within the
/// values' errors `mus`) spans a `nu` extent (with the errors `nus`) below `sweep`.
fn flat_stretches_shorter_than(mus: &[(f64, f64)], nus: &[(f64, f64)], sweep: f64) -> bool {
    let flat = |i: usize| match (mus.get(i), mus.get(i + 1)) {
        (Some(&(m0, e0)), Some(&(m1, e1))) => (m1 - m0).abs() <= e0 + e1,
        _ => false,
    };
    let mut run: Option<(f64, f64)> = None;
    for (i, &(nu, err)) in nus.iter().enumerate() {
        let (lo, hi) = (nu - err, nu + err);
        // (a run holds the points of consecutive flat steps)
        let in_run = i > 0 && flat(i - 1);
        run = match (run, in_run) {
            (Some((a, b)), true) => Some((a.min(lo), b.max(hi))),
            _ => Some((lo, hi)),
        };
        if let Some((a, b)) = run {
            let long = in_run && b - a >= sweep * (1.0 - REL_SLACK);
            if long {
                return false;
            }
        }
    }
    true
}

/// Splits, certifies and measures the generator range `(a, b)`.
fn range_piece(
    k: &Kernel,
    inputs: &PatchInputs<'_>,
    frame: &Frame,
    (a, b): (f64, f64),
    budget: &mut usize,
) -> Result<Measured> {
    let gen = inputs.gen;
    let lo = a.min(b).max(inputs.gen_ivl.min());
    let hi = a.max(b).min(inputs.gen_ivl.max());
    let nonempty = lo < hi;
    if !nonempty {
        return Ok(Measured::Empty);
    }
    let Some((piece, ivl)) = split(k, gen, lo, hi) else {
        return Ok(Measured::Declined);
    };
    let Some(net) = net_points(&piece) else {
        return Ok(Measured::Declined);
    };
    if !certify_in_plane(&net, inputs.extrusion_vec, inputs.plane) {
        return Ok(Measured::Declined);
    }

    // mu monotone on the control polygon (each step not against the direction beyond
    // the values' errors) makes it monotone on the curve: for positive weights the
    // number of roots of mu(C(s)) = c is at most the number of sign changes of
    // mu(P_i) - c (variation diminishing), at most one here, for every c. (A step
    // within the errors may hide a fold no larger than them; a flat stretch is a
    // segment along the sweep, whose copies are the region's boundary there.)
    let mus: Vec<(f64, f64)> = net
        .pts
        .iter()
        .map(|&(p, err)| {
            (
                frame.dmu.dot(p - frame.o),
                functional_err(frame.dmu, p, frame.o, err),
            )
        })
        .collect();
    let (first, last) = (mus.first().map(|m| m.0), mus.last().map(|m| m.0));
    let rising = mus
        .windows(2)
        .all(|w| matches!(w, [(m0, e0), (m1, e1)] if m1 + e1 >= m0 - e0))
        && first < last;
    let falling = mus
        .windows(2)
        .all(|w| matches!(w, [(m0, e0), (m1, e1)] if m1 - e1 <= m0 + e0))
        && last < first;
    // nu over the convex hull of the control points
    let nus: Vec<(f64, f64)> = net
        .pts
        .iter()
        .map(|&(p, err)| {
            (
                frame.dnu.dot(p - frame.o),
                functional_err(frame.dnu, p, frame.o, err),
            )
        })
        .collect();
    let (mut nu_lo, mut nu_hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &(nu, slack) in &nus {
        nu_lo = nu_lo.min(nu - slack);
        nu_hi = nu_hi.max(nu + slack);
    }

    // a flat stretch (control-polygon steps of mu within the errors) runs along the
    // sweep; its two copies bound the region there only while they do not overlap,
    // that is while its nu extent stays below the sweep's length (else the slab's
    // certificate, or the decline)
    let sweep = inputs.v_ivl.max() - inputs.v_ivl.min();
    let short_flats = flat_stretches_shorter_than(&mus, &nus, sweep);
    let monotone = mus.len() >= 2 && (rising || falling) && short_flats;

    // the extremes of mu: at the ends when monotone, else the certified search
    let mu_at = |s: f64| -> Result<f64> {
        Ok(frame.mu(inputs
            .plane
            .project_point_to_uv_domain(gen.evaluate_point(k, s)?)))
    };
    let (alpha, beta) = if monotone {
        let (m0, m1) = (mu_at(ivl.min())?, mu_at(ivl.max())?);
        (m0.min(m1), m0.max(m1))
    } else {
        let width = extreme_width(&mus);
        let lo = extreme(k, inputs, frame, &ivl, 1.0, width, budget)?;
        let hi = extreme(k, inputs, frame, &ivl, -1.0, width, budget)?;
        let (Some(lo), Some(hi)) = (lo, hi) else {
            return Ok(Measured::Declined);
        };
        (lo, -hi)
    };
    let spread = alpha < beta && alpha.is_finite() && beta.is_finite();
    if !spread {
        // (a range of constant mu sweeps no area)
        return Ok(Measured::Declined);
    }
    Ok(Measured::Piece(RangePiece {
        ivl,
        alpha,
        beta,
        monotone,
        nu_lo,
        nu_hi,
    }))
}

/// What lies just past a range end along the generator, from [`past_end`].
enum Past {
    /// The next span, to this parameter, is certified in the plane: the range
    /// extends over it.
    Extends(f64),
    /// The generator leaves the plane: past a parameter whose generator point is
    /// within the tolerance of the end, it is farther from the plane than the
    /// in-plane bound. A point solution within `zone` of the end (in parameter) may
    /// be where the generator is within the solver's tolerance of the plane: that
    /// same departure (infinite: anywhere past the end).
    Departs { zone: f64 },
    /// No certificate either way: the patch declines.
    Declined,
}

/// The least `t` in `[0, 1]` past which `G(t) = (a t^j - eps t) / w - c > x` holds on
/// all of `[t, 1]` (`G` is convex for `j >= 2`, linear for `j = 1`, `G(0) = -c <= x`:
/// one crossing), or `None` when `G(1) <= x`. By bisection, rounded up.
fn crossing(g: impl Fn(f64) -> f64, x: f64) -> Option<f64> {
    let above = g(1.0) > x;
    if !above {
        return None;
    }
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..200 {
        let m = lo + (hi - lo) / 2.0;
        let interior = lo < m && m < hi;
        if !interior {
            break;
        }
        if g(m) > x {
            hi = m;
        } else {
            lo = m;
        }
    }
    Some(hi)
}

/// The certificate past the range end `end`, going `dir` (`1` or `-1`) along the
/// generator. The span `Q` from `end` to the next knot (the generator must be a
/// B-spline curve) is split exactly. Certified in the plane: the range extends over
/// it. Else `Q` must be one Bezier span: with `f = n . (C - O)` (the distance to the
/// plane) its control values `c_i` (from the end outward, weights `w_i`) and
/// `D_i = w_i (c_i - c_0)`, when every `D_i` from an index `j` on has one certain sign
/// with `|D_i| >= a` and those before it `|D_i| <= eps`, then (Bernstein: the terms
/// `i >= j` sum to at least `t^j`, the others to at most `1 - (1 - t)^p <= p t`)
/// `|f(t)| >= (a t^j - eps p t) / w_max - |c_0|`, which exceeds `x` past `t_x`
/// (`crossing`), and the generator over `[0, t]` stays within
/// `p t max_i w_i |P_i - P_0| / w_min` of the end point. The departure needs
/// `t_B < 1` for the in-plane bound `B` of `certify_in_plane` and the end point within
/// the tolerance of the point at `t_B`; its zone is `t_tol` (the solver's tolerance).
fn past_end(k: &Kernel, inputs: &PatchInputs<'_>, end: f64, dir: f64) -> Result<Past> {
    let gen = inputs.gen;
    let CurveKind::BSpline(bs) = gen.kind() else {
        return Ok(Past::Declined);
    };
    let (ivl_lo, ivl_hi) = (inputs.gen_ivl.min(), inputs.gen_ivl.max());
    let gap = EFF_ZERO_PARAM * (1.0 + end.abs());
    let knots = bs.knots(None)?.0;
    let next = if dir > 0.0 {
        knots
            .iter()
            .copied()
            .filter(|&t| t > end + gap)
            .fold(ivl_hi, f64::min)
    } else {
        knots
            .iter()
            .copied()
            .filter(|&t| t < end - gap)
            .fold(ivl_lo, f64::max)
    };
    let (a, b) = if dir > 0.0 { (end, next) } else { (next, end) };
    let Some((piece, made)) = split(k, gen, a, b) else {
        return Ok(Past::Declined);
    };
    let Some(net) = net_points(&piece) else {
        return Ok(Past::Declined);
    };
    if certify_in_plane(&net, inputs.extrusion_vec, inputs.plane) {
        return Ok(Past::Extends(next));
    }
    let p = net.degree;
    let bezier = p >= 1 && net.pts.len() == p + 1 && net.weights.len() == p + 1;
    if !bezier {
        return Ok(Past::Declined);
    }
    let n = inputs.plane.position().z_axis();
    let o = inputs.plane.position().origin();
    // from the end outward
    let mut cps: Vec<(Point3, f64, f64)> = net
        .pts
        .iter()
        .zip(&net.weights)
        .map(|(&(q, err), &w)| (q, err, w))
        .collect();
    if dir < 0.0 {
        cps.reverse();
    }
    let vals: Vec<(f64, f64)> = cps
        .iter()
        .map(|&(q, err, _)| (n.dot(q - o), functional_err(n, q, o, err)))
        .collect();
    let (Some(&(c0, e0)), Some(&(p0, pe0, _))) = (vals.first(), cps.first()) else {
        return Ok(Past::Declined);
    };
    let c0_abs = c0.abs() + e0;
    // D_i with its error (that of c_i - c_0, and the weight's)
    let ws = 1.0 + net.weight_err;
    let diffs: Vec<(f64, f64)> = vals
        .iter()
        .zip(&cps)
        .skip(1)
        .map(|(&(c, e), &(_, _, w))| {
            let d = c - c0;
            (w * d, w * ws * (e + e0 + gamma(2) * (c.abs() + c0.abs())))
        })
        .collect();
    let Some(&(d_last, _)) = diffs.last() else {
        return Ok(Past::Declined);
    };
    let outward = d_last > 0.0;
    // the tail of certain, common signs
    let mut j = diffs.len();
    for (i, &(d, err)) in diffs.iter().enumerate().rev() {
        let certain = d.abs() > err && (d > 0.0) == outward;
        if !certain {
            break;
        }
        j = i;
    }
    let tail = diffs.get(j..).unwrap_or_default();
    let head = diffs.get(..j).unwrap_or_default();
    if tail.is_empty() {
        return Ok(Past::Declined);
    }
    let a_lo = tail
        .iter()
        .map(|&(d, err)| d.abs() - err)
        .fold(f64::INFINITY, f64::min);
    let eps = head
        .iter()
        .map(|&(d, err)| d.abs() + err)
        .fold(0.0_f64, f64::max);
    let (w_min, w_max) = net
        .weights
        .iter()
        .fold((f64::INFINITY, 0.0_f64), |(lo, hi), &w| {
            (lo.min(w), hi.max(w))
        });
    let (w_min, w_max) = (w_min / ws, w_max * ws);
    // (j counts from the first difference, D_1)
    let power = j + 1;
    let pf = p as f64;
    let lower = |t: f64| (a_lo * t.powi(power as i32) - eps * pf * t) / w_max - c0_abs;
    // the extent of the generator over [0, t] from the end point
    let reach = cps
        .iter()
        .skip(1)
        .map(|&(q, err, w)| w * ws * ((q - p0).length() * (1.0 + gamma(8)) + err + pe0))
        .fold(0.0_f64, f64::max)
        / w_min;
    let extent = |t: f64| pf * t * reach;
    let bound = EFF_ZERO * net.scale();
    let Some(t_b) = crossing(lower, bound) else {
        return Ok(Past::Declined);
    };
    let departs = t_b < 1.0 && extent(t_b) <= inputs.tol;
    if !departs {
        return Ok(Past::Declined);
    }
    let zone = match crossing(lower, inputs.tol) {
        Some(t) if t < 1.0 => t * (made.max() - made.min()),
        _ => f64::INFINITY,
    };
    Ok(Past::Departs { zone })
}

/// What bounds a monotone region's slice from below or above: a copy of the
/// generator at an end of `V`, or a side of the domain.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Bound {
    Copy,
    Side(Side),
}

/// A run of the swept boundary: its bound over the generator parameters
/// `[a, b]`, with the `mu` at either end (for a side's end points).
struct Run {
    bound: Bound,
    a: f64,
    b: f64,
    mu_a: f64,
    mu_b: f64,
}

/// Adds the interval `[a, b]` to the runs, extending the last run of the same bound
/// that ends at `a`.
fn push_run(runs: &mut Vec<Run>, bound: Bound, a: (f64, f64), b: (f64, f64)) {
    if let Some(last) = runs.last_mut() {
        if last.bound == bound && last.b == a.0 {
            last.b = b.0;
            last.mu_b = b.1;
            return;
        }
    }
    runs.push(Run {
        bound,
        a: a.0,
        b: b.0,
        mu_a: a.1,
        mu_b: b.1,
    });
}

/// The boundary of the region of one range of monotone `mu` (see the module
/// documentation), or `None` when a search declines or a root is missing (the caller
/// then declines).
fn monotone_region(
    k: &Kernel,
    inputs: &PatchInputs<'_>,
    frame: &Frame,
    rp: &RangePiece,
    budget: &mut usize,
    out: &mut Vec<Curve>,
) -> Result<Option<()>> {
    let gen = inputs.gen;
    let plane = inputs.plane;
    let e = inputs.extrusion_vec;
    let dom = inputs.plane_domain;
    let (vmin, vmax) = (inputs.v_ivl.min(), inputs.v_ivl.max());
    let (s0, s1) = (rp.ivl.min(), rp.ivl.max());
    let uv_at = |s: f64| -> Result<Point2> {
        Ok(plane.project_point_to_uv_domain(gen.evaluate_point(k, s)?))
    };
    let sides = sides_of(&dom);
    let parallel = |side: &Side| frame.e_along(side.axis) == 0.0;

    // the breaks: the range ends, where mu passes a corner of the domain, and where a
    // copy crosses a side of the domain not parallel to the sweep
    let mut breaks: Vec<(f64, Option<f64>)> = vec![(s0, None), (s1, None)];
    let corners = [
        Point2::new(dom.min().x, dom.min().y),
        Point2::new(dom.max().x, dom.min().y),
        Point2::new(dom.min().x, dom.max().y),
        Point2::new(dom.max().x, dom.max().y),
    ];
    for c in corners {
        let mu = frame.mu(c);
        let g = Affine {
            d: frame.dmu,
            o: frame.o,
            c: mu,
        };
        let Some(rs) = roots(k, gen, &rp.ivl, g, budget)? else {
            return Ok(None);
        };
        for r in rs {
            breaks.push((r, Some(mu)));
        }
    }
    for side in sides.iter().filter(|s| !parallel(s)) {
        let d = if side.axis == 0 { frame.du } else { frame.dw };
        let ea = frame.e_along(side.axis);
        for v in [vmin, vmax] {
            let g = Affine {
                d,
                o: frame.o,
                c: side.value - v * ea,
            };
            let Some(rs) = roots(k, gen, &rp.ivl, g, budget)? else {
                return Ok(None);
            };
            for r in rs {
                breaks.push((r, None));
            }
        }
    }
    breaks.retain(|b| s0 <= b.0 && b.0 <= s1);
    breaks.sort_by(|p, q| p.0.total_cmp(&q.0));
    // (one break per parameter, keeping a corner's exact mu)
    let mut merged: Vec<(f64, Option<f64>)> = Vec::new();
    for (s, mu) in breaks {
        match merged.last_mut() {
            Some(last) if last.0 == s => {
                if last.1.is_none() {
                    last.1 = mu;
                }
            }
            _ => merged.push((s, mu)),
        }
    }
    let mut with_mu = Vec::new();
    for (s, mu) in merged {
        let mu = match mu {
            Some(m) => m,
            None => frame.mu(uv_at(s)?),
        };
        with_mu.push((s, mu));
    }

    // the sweep: between breaks the slice's bounds keep their kind
    let mut lower: Vec<Run> = Vec::new();
    let mut upper: Vec<Run> = Vec::new();
    for w in with_mu.windows(2) {
        let [a, b] = w else {
            continue;
        };
        let increasing = a.0 < b.0;
        if !increasing {
            continue;
        }
        let uvm = uv_at((a.0 + b.0) / 2.0)?;
        let Some((dlo, lo_side, dhi, hi_side)) = chord(frame, &dom, uvm) else {
            continue;
        };
        // (each slack relative to the two values compared: a huge sweep does not
        // swallow a small domain)
        let slack = |x: f64, y: f64| REL_SLACK * x.abs().max(y.abs());
        let (lo_v, lo_bound) = match lo_side {
            Some(side) if dlo >= vmin - slack(dlo, vmin) => (dlo.max(vmin), Bound::Side(side)),
            _ => (vmin, Bound::Copy),
        };
        let (hi_v, hi_bound) = match hi_side {
            Some(side) if dhi <= vmax + slack(dhi, vmax) => (dhi.min(vmax), Bound::Side(side)),
            _ => (vmax, Bound::Copy),
        };
        let open = lo_v < hi_v - slack(lo_v, hi_v);
        if !open {
            continue;
        }
        push_run(&mut lower, lo_bound, *a, *b);
        push_run(&mut upper, hi_bound, *a, *b);
    }

    // the runs' curves: copies of the generator (split exactly, as certified), or
    // segments of the sides
    for (runs, v) in [(&lower, vmin), (&upper, vmax)] {
        for run in runs {
            match run.bound {
                Bound::Copy => {
                    let Some((mut copy, _)) = split(k, gen, run.a, run.b) else {
                        return Ok(None);
                    };
                    let mut shift = Axis2Placement::IDENTITY;
                    shift.translate(e * v);
                    ignore_status(copy.transform(k, &shift, None))?;
                    out.push(copy);
                }
                Bound::Side(side) => {
                    let pa = plane.evaluate_point_fast(side_point(frame, &dom, side, run.mu_a));
                    let pb = plane.evaluate_point_fast(side_point(frame, &dom, side, run.mu_b));
                    out.extend(segment(pa, pb)?);
                }
            }
        }
    }

    // across: the directrices at the range ends, and the sides parallel to the sweep
    // between them, cut to the domain. A side within the slack of an end's mu stands
    // for that end's directrix (the side wins the tie, as against the copies): the
    // directrix may lie just outside the domain, and the side bounds the region there.
    let (mu0, mu1) = (rp.alpha, rp.beta);
    let side_mu = |side: &Side| {
        frame.mu(if side.axis == 0 {
            Point2::new(side.value, 0.0)
        } else {
            Point2::new(0.0, side.value)
        })
    };
    let slack_of = |mu: f64| EFF_ZERO * mu0.abs().max(mu1.abs()).max(mu.abs());
    for s in [s0, s1] {
        let p = gen.evaluate_point(k, s)?;
        let mu_s = frame.mu(plane.project_point_to_uv_domain(p));
        let at_side = sides.iter().find(|side| {
            let mu = side_mu(side);
            parallel(side) && (mu - mu_s).abs() <= slack_of(mu)
        });
        out.extend(directrix(inputs, frame, p, at_side.copied())?);
    }
    for side in sides.iter().filter(|s| parallel(s)) {
        let mu = side_mu(side);
        let slack = slack_of(mu);
        let inside = mu0 + slack < mu && mu < mu1 - slack;
        if !inside {
            continue;
        }
        let g = Affine {
            d: frame.dmu,
            o: frame.o,
            c: mu,
        };
        let Some(rs) = roots(k, gen, &rp.ivl, g, budget)? else {
            return Ok(None);
        };
        let [r] = rs.as_slice() else {
            // (monotone, the side strictly inside: exactly one; else decline)
            return Ok(None);
        };
        let p = gen.evaluate_point(k, *r)?;
        out.extend(directrix(inputs, frame, p, Some(*side))?);
    }
    Ok(Some(()))
}

/// The boundary of the region of ranges whose `mu` ranges overlap, on the slab's
/// cover certificate (see the module documentation), or `None` without it.
fn slab_region(
    inputs: &PatchInputs<'_>,
    frame: &Frame,
    group: &[&RangePiece],
    out: &mut Vec<Curve>,
) -> Result<Option<()>> {
    let dom = inputs.plane_domain;
    let (mut alpha, mut beta) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut nu_lo, mut nu_hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for rp in group {
        alpha = alpha.min(rp.alpha);
        beta = beta.max(rp.beta);
        nu_lo = nu_lo.min(rp.nu_lo);
        nu_hi = nu_hi.max(rp.nu_hi);
    }
    // the domain box cut to alpha <= mu <= beta (Sutherland-Hodgman; a vertex on a
    // side of the box keeps that side's coordinate exactly)
    let mut poly = vec![
        Point2::new(dom.min().x, dom.min().y),
        Point2::new(dom.max().x, dom.min().y),
        Point2::new(dom.max().x, dom.max().y),
        Point2::new(dom.min().x, dom.max().y),
    ];
    for (bound, keep_above) in [(alpha, true), (beta, false)] {
        let inside = |p: Point2| {
            let mu = frame.mu(p);
            if keep_above {
                mu >= bound
            } else {
                mu <= bound
            }
        };
        let mut next = Vec::new();
        // (each vertex with its successor, the last with the first)
        for (&p, &q) in poly.iter().zip(poly.iter().cycle().skip(1)) {
            let (pin, qin) = (inside(p), inside(q));
            if pin {
                next.push(p);
            }
            if pin != qin {
                let (mp, mq) = (frame.mu(p), frame.mu(q));
                let t = (bound - mp) / (mq - mp);
                next.push(Point2::new(p.x + t * (q.x - p.x), p.y + t * (q.y - p.y)));
            }
        }
        poly = next;
    }
    if poly.is_empty() {
        // the slab misses the domain: no coincidence over it
        return Ok(Some(()));
    }
    // a slab touching the domain along a side or at a point bounds no area
    let mut area2 = 0.0;
    let mut scale = 0.0_f64;
    // (each vertex with its successor, the last with the first)
    for (&p, &q) in poly.iter().zip(poly.iter().cycle().skip(1)) {
        area2 += p.x * q.y - q.x * p.y;
        scale = scale.max(p.x.abs()).max(p.y.abs());
    }
    let flat = area2.abs() <= EFF_ZERO * scale * scale;
    if flat {
        return Ok(None);
    }
    // the cover: every nu of the polygon (its vertices bound it) within
    // [nu_hi + vmin, nu_lo + vmax], so every slice of the image covers the domain's
    let (vmin, vmax) = (inputs.v_ivl.min(), inputs.v_ivl.max());
    for &p in &poly {
        let nu = frame.nu(p);
        let slack =
            REL_SLACK * (nu.abs() + nu_lo.abs().max(nu_hi.abs()) + vmin.abs().max(vmax.abs()));
        let covered = nu - nu_hi >= vmin - slack && nu - nu_lo <= vmax + slack;
        if !covered {
            return Ok(None);
        }
    }
    // (each vertex with its successor, the last with the first)
    for (&p, &q) in poly.iter().zip(poly.iter().cycle().skip(1)) {
        let (a, b) = (
            inputs.plane.evaluate_point_fast(p),
            inputs.plane.evaluate_point_fast(q),
        );
        out.extend(segment(a, b)?);
    }
    Ok(Some(()))
}

/// The solver's ranges cut to the generator interval, each extended span by span
/// over the spans certified in the plane past its ends, then merged where they meet;
/// `None` when an extension does not end.
fn extended_ranges(
    k: &Kernel,
    inputs: &PatchInputs<'_>,
    ranges: &[(f64, f64)],
) -> Result<Option<Vec<(f64, f64)>>> {
    let (ivl_lo, ivl_hi) = (inputs.gen_ivl.min(), inputs.gen_ivl.max());
    let steps = match inputs.gen.kind() {
        CurveKind::BSpline(bs) => bs.knots(None)?.0.len() + 2,
        _ => 2,
    };
    let mut spans = Vec::new();
    for &(a, b) in ranges {
        let (mut lo, mut hi) = (a.min(b).max(ivl_lo), a.max(b).min(ivl_hi));
        let nonempty = lo < hi;
        if !nonempty {
            continue;
        }
        for dir in [-1.0, 1.0] {
            let mut ended = false;
            for _ in 0..steps {
                let end = if dir < 0.0 { lo } else { hi };
                let inside = ivl_lo < end && end < ivl_hi;
                if !inside {
                    ended = true;
                    break;
                }
                match past_end(k, inputs, end, dir)? {
                    Past::Extends(t) if dir < 0.0 => lo = t,
                    Past::Extends(t) => hi = t,
                    Past::Departs { .. } | Past::Declined => {
                        ended = true;
                        break;
                    }
                }
            }
            if !ended {
                return Ok(None);
            }
        }
        spans.push((lo, hi));
    }
    spans.sort_by(|p, q| p.0.total_cmp(&q.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in spans {
        match merged.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => merged.push((lo, hi)),
        }
    }
    Ok(Some(merged))
}

/// The boundary curves of the coincident region of the generator `ranges` (each
/// `(s0, s1)`) swept over `V`, cut to the plane's domain, with the ranges covered
/// (extended over the spans in the plane past them and merged), or `None` when no
/// certificate covers them (the caller then needs the general intersection). Every
/// range end inside the generator interval (or at the seam of a closed generator)
/// must be certified to leave the plane (see `past_end`). `points` are the generator
/// parameters of the point solutions outside the solver's ranges: those inside the
/// covered ranges are absorbed; one in a departure's zone declines (it is that
/// departure); the others' directrices are lines of constant `mu`, so one whose `mu`
/// is within a region's `[alpha, beta]` (with the solver's tolerance) would run
/// through the region or along its edge, and the region declines.
///
/// # Errors
///
/// Those of the generator's copies, trims and evaluations.
pub(super) fn coincident_patch(
    k: &Kernel,
    inputs: &PatchInputs<'_>,
    ranges: &[(f64, f64)],
    points: &[f64],
) -> Result<Option<Patch>> {
    let (vmin, vmax) = (inputs.v_ivl.min(), inputs.v_ivl.max());
    let sweeps = vmin < vmax && vmin.is_finite() && vmax.is_finite();
    if !sweeps {
        return Ok(None);
    }
    let Some(frame) = Frame::new(inputs.plane, inputs.extrusion_vec) else {
        return Ok(None);
    };
    let mut budget = PIECE_BUDGET;

    // the ranges covered, and the departures past their ends: (end, direction, zone)
    let Some(covered) = extended_ranges(k, inputs, ranges)? else {
        return Ok(None);
    };
    let (ivl_lo, ivl_hi) = (inputs.gen_ivl.min(), inputs.gen_ivl.max());
    let whole = matches!(covered.as_slice(), [(lo, hi)] if *lo <= ivl_lo && *hi >= ivl_hi);
    let mut departures: Vec<(f64, f64, f64)> = Vec::new();
    for &(lo, hi) in &covered {
        for (end, dir) in [(lo, -1.0), (hi, 1.0)] {
            let inside = ivl_lo < end && end < ivl_hi;
            // (at the seam of a closed generator, the span past it starts at the
            // interval's other end)
            let (from, dir) = if inside {
                (end, dir)
            } else if inputs.gen_closed && !whole {
                (if dir < 0.0 { ivl_hi } else { ivl_lo }, dir)
            } else {
                continue;
            };
            match past_end(k, inputs, from, dir)? {
                Past::Departs { zone } => departures.push((from, dir, zone)),
                // (in the plane past the seam: the range wraps around it)
                Past::Extends(_) | Past::Declined => return Ok(None),
            }
        }
    }

    // split, certify and measure every range first: one failure keeps today's answer
    let mut pieces = Vec::new();
    for &r in &covered {
        match range_piece(k, inputs, &frame, r, &mut budget)? {
            Measured::Declined => return Ok(None),
            Measured::Empty => {}
            Measured::Piece(p) => pieces.push(p),
        }
    }
    pieces.sort_by(|p, q| p.alpha.total_cmp(&q.alpha));

    // the groups of overlapping mu ranges (ranges merely touching decline: their
    // regions would share a directrix)
    let mut groups: Vec<Vec<&RangePiece>> = Vec::new();
    let mut reach = f64::NEG_INFINITY;
    for p in &pieces {
        let slack = EFF_ZERO * reach.abs().max(p.alpha.abs());
        match groups.last_mut() {
            Some(g) if p.alpha <= reach => g.push(p),
            Some(_) if p.alpha <= reach + slack => return Ok(None),
            _ => groups.push(vec![p]),
        }
        reach = reach.max(p.beta);
    }

    // the point solutions: absorbed inside the covered ranges, declining in a
    // departure's zone, else their directrices off every region (a point found within
    // `tol` in space is within `tol |dmu|` of its mu)
    for &t in points {
        let absorbed = covered
            .iter()
            .any(|&(lo, hi)| lo - EFF_ZERO_PARAM <= t && t <= hi + EFF_ZERO_PARAM);
        if absorbed {
            continue;
        }
        let departing = departures.iter().any(|&(from, dir, zone)| {
            let past = (t - from) * dir;
            past >= 0.0 && past <= zone
        });
        if departing || !t.is_finite() {
            return Ok(None);
        }
        let mu = frame.mu(inputs
            .plane
            .project_point_to_uv_domain(inputs.gen.evaluate_point(k, t)?));
        for g in &groups {
            let alpha = g.iter().map(|p| p.alpha).fold(f64::INFINITY, f64::min);
            let beta = g.iter().map(|p| p.beta).fold(f64::NEG_INFINITY, f64::max);
            let slack = inputs.tol * frame.dmu.length()
                + EFF_ZERO * alpha.abs().max(beta.abs()).max(mu.abs());
            let on_region = alpha - slack <= mu && mu <= beta + slack;
            if on_region || !mu.is_finite() {
                return Ok(None);
            }
        }
    }

    let mut out: Vec<Curve> = Vec::new();
    for g in &groups {
        let answered = match g.as_slice() {
            [rp] if rp.monotone => monotone_region(k, inputs, &frame, rp, &mut budget, &mut out)?,
            _ => slab_region(inputs, &frame, g, &mut out)?,
        };
        if answered.is_none() {
            return Ok(None);
        }
    }
    Ok(Some(Patch {
        curves: out
            .into_iter()
            .map(|c| (c, TsectCurveType::RegionBoundary))
            .collect(),
        ranges: covered,
    }))
}
