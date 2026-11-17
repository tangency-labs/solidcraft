//! The coincident patch of a surface of extrusion lying in a plane: when the
//! extrusion vector is perpendicular to the plane normal and a range of the generator
//! lies in the plane, the surfaces coincide over a 2D region. Its boundary curves,
//! typed `RegionBoundary`, are the intersection's answer, as for the extrusion /
//! extrusion and revolution special cases.
//!
//! Upstream: none (not upstream: `SmSurfOfExtrusion::IntersectWithPlane` asks for the
//! general intersection here, whose answer then depends on its tolerance).
//!
//! | Rust                   | Upstream |
//! |------------------------|----------|
//! | `coincident_patch` (crate) | none |
//!
//! Deviations: not a port. The patch, in the extrusion's parameters, is the
//! rectangle `[s0, s1] x V` (`s` a generator range, `V` the extrusion interval) cut to
//! the preimage of the plane's domain `D`. The answer is the image of its boundary,
//! as the extrusion / extrusion special case answers: the two generator copies at the
//! ends of `V` and the two directrices at `s0` and `s1`, cut to `D`, and the pieces of
//! `D`'s sides whose preimage lies in the rectangle. (Where the generator turns back
//! along the sweep its patch covers part of itself, and some of these curves lie
//! inside the patch's image in space.) The patch is accepted only on a
//! certificate (see [`certify_in_plane`]); otherwise the caller keeps the general
//! intersection. Every loop is bounded by the number of ranges, domain sides and
//! solver roots; nothing recurses.

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
use crate::math::{EFF_ZERO, EFF_ZERO_PARAM};
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
    /// Whether the generator is closed over `gen_ivl`.
    pub is_closed: bool,
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

/// The certificate that the patch over the generator range `piece` (already split
/// to the range) lies in the plane, in two parts. The sweep: `|n . E| + err <=
/// SM_EFF_ZERO |E|` (`E` in the plane to the working precision). The profile: every
/// Euclidean control point `P` of `piece` has `|n . (P - O)| + err <= SM_EFF_ZERO
/// (1 + |P|)`. With positive weights a rational curve lies in the convex hull of its
/// Euclidean control points, so every point `C(s)` of the range is within the
/// largest of those bounds of the plane, and every patch point `C(s) + v E` within
/// that plus `|v| SM_EFF_ZERO |E|`: the scale of the point itself. `err` counts the
/// rounding of the split (at most `2p` knot insertions of three operations each, on
/// homogeneous coordinates), of the division by the weight, of `P - O`, of the dot
/// products and of the plane normal. Non-finite data and a non-positive weight fail
/// (no NaN passes the comparisons).
fn certify_in_plane(piece: &Curve, e: Vec3, plane: &Plane) -> bool {
    let Ok(base) = gen_base(piece) else {
        return false;
    };
    let Ok(canon) = base.canonical() else {
        return false;
    };
    let n = plane.position().z_axis();
    let o = plane.position().origin();
    let weights_of = |i: usize| -> f64 {
        if canon.weights.is_empty() {
            1.0
        } else {
            canon.weights.get(i).copied().unwrap_or(f64::NAN)
        }
    };
    // largest homogeneous magnitude and weight over the net (the split's operands)
    let mut max_hom = 0.0_f64;
    let mut max_w = 0.0_f64;
    for (i, p) in canon.control_points.iter().enumerate() {
        let w = weights_of(i);
        if !(w > 0.0 && w.is_finite()) {
            return false;
        }
        max_hom = max_hom.max(w * norm1(*p));
        max_w = max_w.max(w);
    }
    if canon.control_points.is_empty() || !max_hom.is_finite() {
        return false;
    }
    let split_ops = 6 * canon.degree + 4;
    let n1 = norm1(n);
    // the sweep (a dot product of three terms; the normal's rounding in n1)
    let ne = n.dot(e);
    let err_e = n1 * gamma(4) * norm1(e);
    let sweep_in_plane = ne.abs() + err_e <= EFF_ZERO * e.max_dimension();
    if !sweep_in_plane {
        return false;
    }
    for (i, &p) in canon.control_points.iter().enumerate() {
        let w = weights_of(i);
        // the split's homogeneous error and the weight's, divided by the weight
        let err_p = gamma(split_ops) * (max_hom + norm1(p) * max_w) / w;
        let dist = n.dot(p - o);
        let err = n1 * (err_p + gamma(8) * (norm1(p) + norm1(o)));
        let bound = EFF_ZERO * (1.0 + p.max_dimension());
        // (a NaN anywhere fails the check)
        let within = dist.abs() + err <= bound;
        if !within {
            return false;
        }
    }
    true
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
    if !(len > 0.0 && len.is_finite() && value.is_finite()) {
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
    if !(len > 0.0 && len.is_finite()) {
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

/// The segment `a`-`b` (in the plane) cut to the plane domain as a curve (a
/// degenerate one when its length is within `SM_EFF_ZERO` of zero, relative), or
/// `None` outside the domain. The cut is made in the plane's UV coordinates (no
/// tolerance): an end cut by a side of the domain has that side's coordinate
/// exactly, so it meets the curves along that side however long the segment. A
/// segment along a side (`on_side`: the UV coordinate, 0 = u or 1 = v, and its
/// value) has that coordinate exactly, and is cut along the other one only.
fn trimmed_segment(
    inputs: &PatchInputs<'_>,
    a: Point3,
    b: Point3,
    on_side: Option<(usize, f64)>,
) -> Result<Option<Curve>> {
    let plane = inputs.plane;
    let dom = &inputs.plane_domain;
    let (mut ua, mut ub) = (
        plane.project_point_to_uv_domain(a),
        plane.project_point_to_uv_domain(b),
    );
    match on_side {
        Some((0, c)) => (ua.x, ub.x) = (c, c),
        Some((_, c)) => (ua.y, ub.y) = (c, c),
        None => {}
    }
    // Liang-Barsky: the parameters entering and leaving the box, and which
    // coordinate (0 = u, 1 = v) and bound fixed them
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
    let end = |t: f64, fix: Option<(usize, f64)>, orig: Point3| -> Point3 {
        match fix {
            None if on_side.is_none() => orig,
            None => plane.evaluate_point(Point2::new(
                ua.x + t * (ub.x - ua.x),
                ua.y + t * (ub.y - ua.y),
            )),
            Some((i, bound)) => {
                let mut uv = Point2::new(ua.x + t * (ub.x - ua.x), ua.y + t * (ub.y - ua.y));
                if i == 0 {
                    uv.x = bound;
                } else {
                    uv.y = bound;
                }
                plane.evaluate_point(uv)
            }
        }
    };
    let (s, e) = (end(t0, fix0, a), end(t1, fix1, b));
    if s.distance(e) <= EFF_ZERO * (1.0 + s.max_dimension()) {
        let mid = (s + e) / 2.0;
        return Ok(Some(Curve::from(BSplineCurve::create_degenerate_curve(
            3, mid,
        )?)));
    }
    Ok(Some(Curve::from(BSplineCurve::create_line_segment(
        3, s, e,
    )?)))
}

/// The boundary curves of the coincident patches of the generator `ranges` (each
/// `(s0, s1)`), cut to the plane's domain, or `None` when a range fails the
/// in-plane certificate (the caller then needs the general intersection).
///
/// # Errors
///
/// Those of the generator's copies, trims, evaluations and property analyses, and of
/// the plane's domain trimming.
pub(super) fn coincident_patch(
    k: &Kernel,
    inputs: &PatchInputs<'_>,
    ranges: &[(f64, f64)],
) -> Result<Option<Vec<(Curve, TsectCurveType)>>> {
    let gen = inputs.gen;
    let e = inputs.extrusion_vec;
    let (vmin, vmax) = (inputs.v_ivl.min(), inputs.v_ivl.max());
    if !(vmin <= vmax && vmin.is_finite() && vmax.is_finite()) {
        return Ok(None);
    }

    // split every range first, and certify it: one failure keeps today's answer
    let mut pieces = Vec::new();
    for &(a, b) in ranges {
        let lo = a.min(b).max(inputs.gen_ivl.min());
        let hi = a.max(b).min(inputs.gen_ivl.max());
        let nonempty = lo < hi;
        if !nonempty {
            continue;
        }
        let mut ivl = Extent1d::from_min_max(lo, hi);
        let mut piece = gen.clone();
        // (may snap the interval to a knot: the certificate covers the piece made)
        ignore_status(piece.trim(k, &mut ivl, TrimMode::Silent))?;
        if !certify_in_plane(&piece, e, inputs.plane) {
            return Ok(None);
        }
        pieces.push((ivl, piece));
    }

    let mut out: Vec<Curve> = Vec::new();
    let position = *inputs.plane.position();
    let o = position.origin();
    let scale = inputs.plane.uv_scale();
    let axis_u = position.x_axis() / scale.x;
    let axis_w = position.y_axis() / scale.y;
    let dom = inputs.plane_domain;

    for (ivl, piece) in &pieces {
        let (s0, s1) = (ivl.min(), ivl.max());

        // the generator copies at the ends of V, cut to the plane domain
        for v in [vmin, vmax] {
            let mut shift = Axis2Placement::IDENTITY;
            shift.translate(e * v);
            let mut copy = piece.clone();
            ignore_status(copy.transform(k, &shift, None))?;
            out.extend(
                inputs
                    .plane
                    .trim_curve_to_plane_domain(k, &dom, &copy, inputs.tol)?,
            );
        }

        // the directrices at the range ends (none across the seam of a closed
        // generator in the plane all round)
        let whole_closed = inputs.is_closed
            && (s0 - inputs.gen_ivl.min()).abs() <= EFF_ZERO_PARAM
            && (s1 - inputs.gen_ivl.max()).abs() <= EFF_ZERO_PARAM;
        if !whole_closed {
            for s in [s0, s1] {
                let p = gen.evaluate_point(k, s)?;
                if let Some(c) = trimmed_segment(inputs, p + e * vmin, p + e * vmax, None)? {
                    out.push(c);
                }
            }
        }

        // the generator's knots inside the range: its tangent may turn there without
        // passing through a perpendicular (a polyline's corners)
        let knots: Vec<f64> = match gen.kind() {
            CurveKind::BSpline(b) => b
                .knots(None)?
                .0
                .into_iter()
                .filter(|&t| t > s0 && t < s1)
                .collect(),
            _ => Vec::new(),
        };

        // the parts of the plane domain's sides inside the patch
        let sides = [
            (axis_u, 0, dom.min().x, axis_w),
            (axis_u, 0, dom.max().x, axis_w),
            (axis_w, 1, dom.min().y, axis_u),
            (axis_w, 1, dom.max().y, axis_u),
        ];
        for (fixed, side_axis, c, running) in sides {
            let on_side = Some((side_axis, c));
            let ef = fixed.dot(e);
            let at = |s: f64| -> Result<f64> { Ok(fixed.dot(gen.evaluate_point(k, s)? - o)) };
            if ef == 0.0 {
                // a side parallel to the sweep: directrices where the generator
                // crosses it, inside the range
                for r in affine_roots(k, gen, ivl, fixed, c + fixed.dot(o), inputs.tol)? {
                    if r - s0 <= EFF_ZERO_PARAM || s1 - r <= EFF_ZERO_PARAM {
                        continue;
                    }
                    let p = gen.evaluate_point(k, r)?;
                    if let Some(cv) = trimmed_segment(inputs, p + e * vmin, p + e * vmax, on_side)?
                    {
                        out.push(cv);
                    }
                }
                continue;
            }
            // the side is v = (c - fixed . (C(s) - O)) / ef; its point moves along the
            // side monotonically between the turns of beta . C(s)
            let v_side = |s: f64| -> Result<f64> { Ok((c - at(s)?) / ef) };
            let beta = running - fixed * (running.dot(e) / ef);
            let mut breaks = vec![s0, s1];
            breaks.extend(affine_roots(
                k,
                gen,
                ivl,
                fixed,
                c + fixed.dot(o) - vmin * ef,
                inputs.tol,
            )?);
            breaks.extend(affine_roots(
                k,
                gen,
                ivl,
                fixed,
                c + fixed.dot(o) - vmax * ef,
                inputs.tol,
            )?);
            breaks.extend(turning_params(k, gen, ivl, beta, inputs.tol)?);
            breaks.extend_from_slice(&knots);
            breaks.sort_by(f64::total_cmp);
            breaks.dedup();

            // the side's pieces: between consecutive breaks the side's v stays on one
            // side of V's ends and its point runs one way along the side, so each piece
            // is a segment (the image of the patch boundary's piece in the extrusion's
            // parameters; pieces of a generator turning back along E overlap in space)
            for w in breaks.windows(2) {
                let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else {
                    continue;
                };
                let increasing = a < b;
                if !increasing {
                    continue;
                }
                let vm = v_side((a + b) / 2.0)?;
                if !(vmin <= vm && vm <= vmax) {
                    continue;
                }
                let point = |s: f64| -> Result<Point3> {
                    // (max/min, not clamp: no panic on a NaN)
                    let v = v_side(s)?.max(vmin).min(vmax);
                    Ok(gen.evaluate_point(k, s)? + e * v)
                };
                let (pa, pb) = (point(a)?, point(b)?);
                // (a piece of no length is no boundary)
                if pa.distance(pb) <= EFF_ZERO * (1.0 + pa.max_dimension()) {
                    continue;
                }
                if let Some(cv) = trimmed_segment(inputs, pa, pb, on_side)? {
                    out.push(cv);
                }
            }
        }
    }

    let mut typed = Vec::new();
    for c in out {
        let t = if c.is_degenerate(k, EFF_ZERO, None)? {
            TsectCurveType::Touching
        } else {
            TsectCurveType::RegionBoundary
        };
        typed.push((c, t));
    }
    Ok(Some(typed))
}
