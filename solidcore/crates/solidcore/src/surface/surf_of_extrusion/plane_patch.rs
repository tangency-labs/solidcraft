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
//!   polygon, see `range_piece`). Then every `mu` but those of flat stretches (along
//!   the sweep, bounded by the copies) meets the range once, the slice of `I` at `mu`
//!   is one interval `[nu(s) + vmin, nu(s) + vmax]`, and the slice of `R` is that
//!   interval cut to the slice of `D`. The boundary is swept along the range: below,
//!   the copy at `vmin` or the side of `D` above it; above, the copy at `vmax` or the
//!   side of `D` below it; across, the directrices at the range ends and the sides of
//!   `D` parallel to the sweep, each cut to `D`. A copy and a side that coincide give
//!   the side only.
//! - **slab**: ranges whose `mu` ranges overlap (one connected `mu` interval
//!   `[alpha, beta]`, folds allowed), when every `nu` of `D` in the slab
//!   `alpha <= mu <= beta` is within `[nu_max + vmin, nu_min + vmax]` (`nu_min`,
//!   `nu_max` bound `nu` over the ranges' control points). Every slice of `R` is then
//!   the slice of `D`, so `R` is the convex polygon `D` cut to the slab, and its edges
//!   are the answer: the fold (or end) directrices at `alpha` and `beta` and the parts
//!   of `D`'s sides between them.
//!
//! Ranges with disjoint `mu` ranges have disjoint regions, answered one by one. A
//! region of no area (a range of constant `mu`, a slab touching `D` at a point)
//! declines. Every loop is bounded by the number of ranges, domain sides and solver
//! roots; nothing recurses.

use crate::curve::curve::p7::PropertyAnalysisOptions;
use crate::curve::curve::{Curve, CurveKind, CurvePropertyType};
use crate::error::{ignore_status, Result};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::solution_array::SolutionType;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::math::gamma;
use crate::surface::plane::Plane;
use crate::surface::surface::p2::TsectCurveType;
use crate::surface::surface::p4::NoObject;

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
    /// The extrusion interval `V` of the caller's domain.
    pub v_ivl: Extent1d,
    /// The plane.
    pub plane: &'a Plane,
    /// The plane's domain `D`.
    pub plane_domain: Extent2d,
    /// The special case's tolerance (`min(approx, SM_EFF_ZERO_SQRT)`).
    pub tol: f64,
}

/// `|x| + |y| + |z|`.
fn norm1(v: Vec3) -> f64 {
    v.x.abs() + v.y.abs() + v.z.abs()
}

/// The relative slack of the comparisons that decide ties and the slab's cover:
/// far above the rounding of the coordinates compared (a few `gamma(8)`), far below
/// any geometric tolerance. Within it a copy and a side of the domain coincide.
const REL_SLACK: f64 = 1e-12;

/// The Euclidean control points of a generator piece with a bound on each one's
/// error from the exact split of the generator: the split's rounding (at most `2p`
/// knot insertions of three operations each, on homogeneous coordinates) and the
/// division by the weight. `None` for non-finite data or a non-positive weight (no
/// NaN passes the comparisons).
fn net_points(piece: &Curve) -> Option<Vec<(Point3, f64)>> {
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
    let mut out = Vec::new();
    for (i, &p) in canon.control_points.iter().enumerate() {
        let w = weights_of(i);
        out.push((p, gamma(split_ops) * (max_hom + norm1(p) * max_w) / w));
    }
    Some(out)
}

/// A bound on the error of `d . (P - O)` computed for a control point `P` known to
/// `err`: the point's error and the rounding of the difference and the dot product.
fn functional_err(d: Vec3, p: Point3, o: Point3, err: f64) -> f64 {
    norm1(d) * (err + gamma(8) * (norm1(p) + norm1(o)))
}

/// The certificate that the patch over the generator piece `net` (its control points
/// with their errors) lies in the plane, in two parts. The sweep: `|n . E| + err <=
/// SM_EFF_ZERO |E|` (`E` in the plane to the working precision). The profile: every
/// Euclidean control point `P` has `|n . (P - O)| + err <= SM_EFF_ZERO (1 + |P|)`.
/// With positive weights a rational curve lies in the convex hull of its Euclidean
/// control points, so every point `C(s)` of the range is within the largest of those
/// bounds of the plane, and every patch point `C(s) + v E` within that plus
/// `|v| SM_EFF_ZERO |E|`: the scale of the point itself. `err` counts the rounding of
/// the split, of `P - O`, of the dot products and of the plane normal (`|n|_1`).
fn certify_in_plane(net: &[(Point3, f64)], e: Vec3, plane: &Plane) -> bool {
    let n = plane.position().z_axis();
    let o = plane.position().origin();
    let n1 = norm1(n);
    let err_e = n1 * gamma(4) * norm1(e);
    let sweep_in_plane = n.dot(e).abs() + err_e <= EFF_ZERO * e.max_dimension();
    if !sweep_in_plane {
        return false;
    }
    net.iter().all(|&(p, err)| {
        let bound = EFF_ZERO * (1.0 + p.max_dimension());
        // (a NaN anywhere fails the check)
        n.dot(p - o).abs() + functional_err(n, p, o, err) <= bound
    })
}

/// The generator parameters in the open interval `(a, b)` where `dir . C(s) = value`
/// (a plane intersection of the generator; the ends of a range solution count).
fn affine_roots(
    k: &Kernel,
    gen: &Curve,
    ivl: &Extent1d,
    dir: Vec3,
    value: f64,
    tol: f64,
) -> Result<Vec<f64>> {
    let len = dir.length();
    let usable = len > 0.0 && len.is_finite() && value.is_finite();
    if !usable {
        return Ok(Vec::new());
    }
    let vecs = [dir / len];
    let opts = PropertyAnalysisOptions::<NoObject> {
        property_value: Some(-value / len),
        opt_vectors: &vecs,
        want_comb: false,
        handle: None,
    };
    let sols = gen
        .global_property_analysis(k, ivl, CurvePropertyType::PlaneIntersection, tol, &opts)?
        .solutions;
    Ok(params_of(sols.as_slice(), ivl))
}

/// The generator parameters in the open interval of `ivl` where `C'(s)` is
/// perpendicular to `dir` (where `dir . C(s)` turns).
fn turning_params(
    k: &Kernel,
    gen: &Curve,
    ivl: &Extent1d,
    dir: Vec3,
    tol: f64,
) -> Result<Vec<f64>> {
    let len = dir.length();
    let usable = len > 0.0 && len.is_finite();
    if !usable {
        return Ok(Vec::new());
    }
    let vecs = [dir / len];
    let opts = PropertyAnalysisOptions::<NoObject> {
        property_value: None,
        opt_vectors: &vecs,
        want_comb: false,
        handle: None,
    };
    let sols = gen
        .global_property_analysis(k, ivl, CurvePropertyType::PerpendicularToVector, tol, &opts)?
        .solutions;
    Ok(params_of(sols.as_slice(), ivl))
}

/// The parameters of solutions strictly inside `ivl` (both ends of a range).
fn params_of<O>(sols: &[crate::geom::solution_array::Solution<O>], ivl: &Extent1d) -> Vec<f64> {
    let mut out = Vec::new();
    for s in sols {
        let mut push = |t: f64| {
            if t > ivl.min() && t < ivl.max() {
                out.push(t);
            }
        };
        if let Some(&t) = s.start.parameters.first() {
            push(t);
        }
        if s.solution_type == SolutionType::RangeOfValues {
            if let Some(&t) = s.end.parameters.first() {
                push(t);
            }
        }
    }
    out
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

/// The segment `a`-`b` (in the plane) cut to the plane domain as a curve, or `None`
/// outside the domain or of no length (within `SM_EFF_ZERO`, relative). The cut is
/// made in the plane's UV coordinates (no tolerance): an end cut by a side of the
/// domain has that side's coordinate exactly, so it meets the curves along that side
/// however long the segment. A segment along a side (`on_side`) has that side's
/// coordinate exactly, and is cut along the other one only.
fn trimmed_segment(
    inputs: &PatchInputs<'_>,
    a: Point3,
    b: Point3,
    on_side: Option<Side>,
) -> Result<Option<Curve>> {
    let plane = inputs.plane;
    let dom = &inputs.plane_domain;
    let (mut ua, mut ub) = (
        plane.project_point_to_uv_domain(a),
        plane.project_point_to_uv_domain(b),
    );
    match on_side {
        Some(Side { axis: 0, value }) => (ua.x, ub.x) = (value, value),
        Some(Side { value, .. }) => (ua.y, ub.y) = (value, value),
        None => {}
    }
    // Liang-Barsky: the parameters entering and leaving the box, and which
    // coordinate (0 = u, 1 = w) and bound fixed them
    let mut t0 = 0.0_f64;
    let mut t1 = 1.0_f64;
    let mut fix0: Option<(usize, f64)> = None;
    let mut fix1: Option<(usize, f64)> = None;
    let axes = [
        (ua.x, ub.x, dom.min().x, dom.max().x),
        (ua.y, ub.y, dom.min().y, dom.max().y),
    ];
    for (i, (pa, pb, lo, hi)) in axes.into_iter().enumerate() {
        let finite = pa.is_finite() && pb.is_finite() && lo.is_finite() && hi.is_finite();
        if !finite {
            return Ok(None);
        }
        let d = pb - pa;
        if d == 0.0 {
            if pa < lo || pa > hi {
                return Ok(None);
            }
            continue;
        }
        let (t_lo, t_hi) = ((lo - pa) / d, (hi - pa) / d);
        let ((enter, enter_at), (leave, leave_at)) = if d > 0.0 {
            ((t_lo, lo), (t_hi, hi))
        } else {
            ((t_hi, hi), (t_lo, lo))
        };
        if enter > t0 {
            t0 = enter;
            fix0 = Some((i, enter_at));
        }
        if leave < t1 {
            t1 = leave;
            fix1 = Some((i, leave_at));
        }
    }
    if t0 > t1 {
        return Ok(None);
    }
    let end = |t: f64, fix: Option<(usize, f64)>| -> Point2 {
        let mut uv = Point2::new(ua.x + t * (ub.x - ua.x), ua.y + t * (ub.y - ua.y));
        match fix {
            Some((0, bound)) => uv.x = bound,
            Some((_, bound)) => uv.y = bound,
            None => {}
        }
        uv
    };
    let (s, e) = (end(t0, fix0), end(t1, fix1));
    // (an unclipped end of a segment off any side is the caller's point)
    let s3 = if fix0.is_none() && on_side.is_none() && t0 == 0.0 {
        a
    } else {
        plane.evaluate_point_fast(s)
    };
    let e3 = if fix1.is_none() && on_side.is_none() && t1 == 1.0 {
        b
    } else {
        plane.evaluate_point_fast(e)
    };
    segment(s3, e3)
}

/// The segment `a`-`b`, or `None` when it has no length (within `SM_EFF_ZERO`,
/// relative).
fn segment(a: Point3, b: Point3) -> Result<Option<Curve>> {
    let short = a.distance(b) <= EFF_ZERO * (1.0 + a.max_dimension());
    if short {
        return Ok(None);
    }
    Ok(Some(Curve::from(BSplineCurve::create_line_segment(
        3, a, b,
    )?)))
}

/// A generator range, split from the generator and certified in the plane, with its
/// `mu` extent (the values and parameters of its least and greatest `mu`), whether
/// its `mu` is certified monotone, and bounds on its `nu`.
struct RangePiece {
    ivl: Extent1d,
    alpha: (f64, f64),
    beta: (f64, f64),
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

/// Splits, certifies and measures the generator range `(a, b)`.
fn range_piece(
    k: &Kernel,
    inputs: &PatchInputs<'_>,
    frame: &Frame,
    (a, b): (f64, f64),
) -> Result<Measured> {
    let gen = inputs.gen;
    let lo = a.min(b).max(inputs.gen_ivl.min());
    let hi = a.max(b).min(inputs.gen_ivl.max());
    let nonempty = lo < hi;
    if !nonempty {
        return Ok(Measured::Empty);
    }
    let mut ivl = Extent1d::from_min_max(lo, hi);
    let mut piece = gen.clone();
    // (may snap the interval to a knot: the certificates cover the piece made)
    ignore_status(piece.trim(k, &mut ivl, TrimMode::Silent))?;
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
    let monotone = mus.len() >= 2 && (rising || falling);

    // nu over the convex hull of the control points
    let (mut nu_lo, mut nu_hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &(p, err) in &net {
        let nu = frame.dnu.dot(p - frame.o);
        let slack = functional_err(frame.dnu, p, frame.o, err);
        nu_lo = nu_lo.min(nu - slack);
        nu_hi = nu_hi.max(nu + slack);
    }

    // the extremes of mu: at the ends, the turns, or the knots (a polyline turns at
    // its corners without a perpendicular)
    let mut cands = vec![ivl.min(), ivl.max()];
    if !monotone {
        let tol = inputs.tol;
        cands.extend(turning_params(k, gen, &ivl, frame.dmu, tol)?);
        if let CurveKind::BSpline(bs) = gen.kind() {
            cands.extend(
                bs.knots(None)?
                    .0
                    .into_iter()
                    .filter(|&t| t > ivl.min() && t < ivl.max()),
            );
        }
    }
    let mut alpha = (f64::INFINITY, f64::NAN);
    let mut beta = (f64::NEG_INFINITY, f64::NAN);
    for s in cands {
        let mu = frame.mu(inputs
            .plane
            .project_point_to_uv_domain(gen.evaluate_point(k, s)?));
        if mu < alpha.0 {
            alpha = (mu, s);
        }
        if mu > beta.0 {
            beta = (mu, s);
        }
    }
    let spread = alpha.0 < beta.0 && alpha.0.is_finite() && beta.0.is_finite();
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
/// documentation), or `None` when a root is missing (the caller then declines).
fn monotone_region(
    k: &Kernel,
    inputs: &PatchInputs<'_>,
    frame: &Frame,
    rp: &RangePiece,
    out: &mut Vec<Curve>,
) -> Result<Option<()>> {
    let gen = inputs.gen;
    let plane = inputs.plane;
    let e = inputs.extrusion_vec;
    let dom = inputs.plane_domain;
    let (vmin, vmax) = (inputs.v_ivl.min(), inputs.v_ivl.max());
    let (s0, s1) = (rp.ivl.min(), rp.ivl.max());
    let tol = inputs.tol;
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
        let value = mu + frame.dmu.dot(frame.o);
        for r in affine_roots(k, gen, &rp.ivl, frame.dmu, value, tol)? {
            breaks.push((r, Some(mu)));
        }
    }
    for side in sides.iter().filter(|s| !parallel(s)) {
        let d = if side.axis == 0 { frame.du } else { frame.dw };
        let ea = frame.e_along(side.axis);
        for v in [vmin, vmax] {
            let value = side.value - v * ea + d.dot(frame.o);
            for r in affine_roots(k, gen, &rp.ivl, d, value, tol)? {
                breaks.push((r, None));
            }
        }
    }
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
        let slack = REL_SLACK * (1.0 + vmin.abs().max(vmax.abs()).max(dlo.abs()).max(dhi.abs()));
        let (lo_v, lo_bound) = match lo_side {
            Some(side) if dlo >= vmin - slack => (dlo.max(vmin), Bound::Side(side)),
            _ => (vmin, Bound::Copy),
        };
        let (hi_v, hi_bound) = match hi_side {
            Some(side) if dhi <= vmax + slack => (dhi.min(vmax), Bound::Side(side)),
            _ => (vmax, Bound::Copy),
        };
        let open = lo_v < hi_v - slack;
        if !open {
            continue;
        }
        push_run(&mut lower, lo_bound, *a, *b);
        push_run(&mut upper, hi_bound, *a, *b);
    }

    // the runs' curves: copies of the generator, or segments of the sides
    for (runs, v) in [(&lower, vmin), (&upper, vmax)] {
        for run in runs {
            match run.bound {
                Bound::Copy => {
                    let mut ivl = Extent1d::from_min_max(run.a, run.b);
                    let mut copy = gen.clone();
                    ignore_status(copy.trim(k, &mut ivl, TrimMode::Silent))?;
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
    // strictly between them, cut to the domain
    for s in [s0, s1] {
        let p = gen.evaluate_point(k, s)?;
        out.extend(trimmed_segment(inputs, p + e * vmin, p + e * vmax, None)?);
    }
    let (mu0, mu1) = (rp.alpha.0, rp.beta.0);
    for side in sides.iter().filter(|s| parallel(s)) {
        let mu = frame.mu(if side.axis == 0 {
            Point2::new(side.value, 0.0)
        } else {
            Point2::new(0.0, side.value)
        });
        let slack = EFF_ZERO * (1.0 + mu0.abs().max(mu1.abs()));
        let inside = mu0 + slack < mu && mu < mu1 - slack;
        if !inside {
            continue;
        }
        let roots = affine_roots(k, gen, &rp.ivl, frame.dmu, mu + frame.dmu.dot(frame.o), tol)?;
        let [r] = roots.as_slice() else {
            // (monotone: exactly one; anything else declines)
            return Ok(None);
        };
        let p = gen.evaluate_point(k, *r)?;
        out.extend(trimmed_segment(
            inputs,
            p + e * vmin,
            p + e * vmax,
            Some(*side),
        )?);
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
        alpha = alpha.min(rp.alpha.0);
        beta = beta.max(rp.beta.0);
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
    let flat = area2.abs() <= EFF_ZERO * (1.0 + scale) * (1.0 + scale);
    if flat {
        return Ok(None);
    }
    // the cover: every nu of the polygon (its vertices bound it) within
    // [nu_hi + vmin, nu_lo + vmax], so every slice of the image covers the domain's
    let (vmin, vmax) = (inputs.v_ivl.min(), inputs.v_ivl.max());
    for &p in &poly {
        let nu = frame.nu(p);
        let slack = REL_SLACK
            * (1.0 + nu.abs() + nu_lo.abs().max(nu_hi.abs()) + vmin.abs().max(vmax.abs()));
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

/// The boundary curves of the coincident region of the generator `ranges` (each
/// `(s0, s1)`) swept over `V`, cut to the plane's domain, or `None` when no
/// certificate covers them (the caller then needs the general intersection).
///
/// # Errors
///
/// Those of the generator's copies, trims, evaluations and property analyses.
pub(super) fn coincident_patch(
    k: &Kernel,
    inputs: &PatchInputs<'_>,
    ranges: &[(f64, f64)],
) -> Result<Option<Vec<(Curve, TsectCurveType)>>> {
    let (vmin, vmax) = (inputs.v_ivl.min(), inputs.v_ivl.max());
    let sweeps = vmin < vmax && vmin.is_finite() && vmax.is_finite();
    if !sweeps {
        return Ok(None);
    }
    let Some(frame) = Frame::new(inputs.plane, inputs.extrusion_vec) else {
        return Ok(None);
    };

    // split, certify and measure every range first: one failure keeps today's answer
    let mut pieces = Vec::new();
    for &r in ranges {
        match range_piece(k, inputs, &frame, r)? {
            Measured::Declined => return Ok(None),
            Measured::Empty => {}
            Measured::Piece(p) => pieces.push(p),
        }
    }
    pieces.sort_by(|p, q| p.alpha.0.total_cmp(&q.alpha.0));

    // the groups of overlapping mu ranges (ranges merely touching decline: their
    // regions would share a directrix)
    let mut groups: Vec<Vec<&RangePiece>> = Vec::new();
    let mut reach = f64::NEG_INFINITY;
    for p in &pieces {
        let slack = EFF_ZERO * (1.0 + reach.abs().max(p.alpha.0.abs()));
        match groups.last_mut() {
            Some(g) if p.alpha.0 <= reach => g.push(p),
            Some(_) if p.alpha.0 <= reach + slack => return Ok(None),
            _ => groups.push(vec![p]),
        }
        reach = reach.max(p.beta.0);
    }

    let mut out: Vec<Curve> = Vec::new();
    for g in &groups {
        let answered = match g.as_slice() {
            [rp] if rp.monotone => monotone_region(k, inputs, &frame, rp, &mut out)?,
            _ => slab_region(inputs, &frame, g, &mut out)?,
        };
        if answered.is_none() {
            return Ok(None);
        }
    }
    Ok(Some(
        out.into_iter()
            .map(|c| (c, TsectCurveType::RegionBoundary))
            .collect(),
    ))
}
