//! An interpolating skin between two matched planar loops, with one prescribed end tangent.
//!
//! The section curves are position constraints. One additional first derivative makes the
//! run a quadratic Bézier. Crown scales one derivative along the section-centre connector.
//! Bound its length by the matched chords: the shortest forward chord must not be retraced.
//! Within a wall, the two section curves have one rational denominator. This keeps the
//! quadratic interpolation in Euclidean space even across a circular arc's internal knots.
//! At a matched vertex the same two points and the same derivative define the same run curve
//! on both adjacent walls.

use crate::blend::fillet_section::interval::Ivl;
use crate::error::{Result, SolidError};
use crate::eval::Frame;
use crate::geom::certified::next_up;
use crate::geom::extent1d::Extent1d;
use crate::geom::interval_vector::{cross, dot, norm};
use crate::geom::tol::USE_DEFAULT;
use crate::math::{Vec3, EFF_ZERO};
use crate::nurbs::b_spline_curve_n_lib::knot_union;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::crv_fit::p1::DerivEnd;
use crate::nurbs::funcs_basic::CurveFunction;
use crate::nurbs::math::CPoint;
use crate::nurbs::nurbs_crv::crv_piece_hull_points;
use crate::nurbs::srf_basic::{Direction, Surface as Net};
use crate::nurbs::srf_tool::p1::insert_knots;
use crate::prism::profile::{axes, Seg2};
use crate::surface::surface::assert_valid::grid_has_non_monotonic_points_to_rounding;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::sweep::loft_sections::{centroid, match_loft_sections, signed_area, LoftSection};
use crate::topology::brep::{p3::new_brep, p4::stitch_faces, sheet::face_on};
use crate::{Brep, FaceId, Kernel};

const OP: &str = "sweep::skin_planar_g1";
const RUN: [f64; 6] = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn section_curve(frame: &Frame, seg: &Seg2) -> Result<Curve> {
    let spans = seg
        .spans()
        .ok_or_else(|| invalid("a malformed sketch segment"))?;
    let degree = spans
        .first()
        .and_then(|s| s.len().checked_sub(1))
        .filter(|&p| p > 0)
        .ok_or_else(|| invalid("a sketch segment has no curve"))?;
    let (origin, x, y, _) = axes(frame);
    let mut knots = vec![0.0; degree + 1];
    let mut points = Vec::new();
    // Every multi-span segment is an arc. Its equal-angle spans have unit end weights,
    // and the two controls at a shared end are the same point by construction.
    #[allow(clippy::cast_precision_loss)] // Seg2::spans has at most eight spans.
    for (i, span) in spans.iter().enumerate() {
        if i != 0 {
            knots.extend(std::iter::repeat_n(i as f64 / spans.len() as f64, degree));
        }
        for &[sx, sy, w] in span.iter().skip(usize::from(i != 0)) {
            let p = origin * w + x * sx + y * sy;
            if !(w > 0.0 && w.is_finite() && p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
            {
                return Err(invalid(
                    "a section control is not finite with positive weight",
                ));
            }
            points.push(CPoint::new(p.x, p.y, p.z, w));
        }
    }
    knots.extend(std::iter::repeat_n(1.0, degree + 1));
    Curve::new(points, degree, knots)
}

/// The same two curves with one denominator. Interpolating unbalanced homogeneous curves
/// can turn even a circle's smooth join into a crease in the surface: its weight derivative
/// jumps there but the other section's need not. Cross multiply by the other denominator,
/// so the surface instead blends the Euclidean points, not their relative weights.
fn compatible([mut a, mut b]: [Curve; 2]) -> Result<[Curve; 2]> {
    let weights = |c: &Curve| {
        CurveFunction::new(
            c.cpts().iter().map(|p| p.w).collect(),
            c.degree(),
            c.knots().to_vec(),
        )
    };
    let unit = |c: &Curve| c.cpts().iter().all(|p| p.w == 1.0);
    let [unit_a, unit_b] = [unit(&a), unit(&b)];
    if !unit_a || !unit_b {
        // Interior knots here are i/n for at most eight arc spans. Distinct
        // sites cannot be mistaken for each other by the product's knot tolerance.
        let w_a = weights(&a)?;
        let w_b = weights(&b)?;
        if !unit_a
            && !unit_b
            && (a.degree() != b.degree() || a.knots() != b.knots() || w_a.values() != w_b.values())
        {
            let a_wb = w_b.clone().multiply_crv_4d(&mut a)?;
            let b_wa = w_a.clone().multiply_crv_4d(&mut b)?;
            a = a_wb;
            b = b_wa;
        } else if unit_a && !unit_b {
            a = w_b.clone().multiply_crv_4d(&mut a)?;
        } else if unit_b && !unit_a {
            b = w_a.clone().multiply_crv_4d(&mut b)?;
        }
    }
    let [a, mut b] = common_basis([a, b])?;
    // These are coefficients of the same function in the same basis: only the
    // rounding of the products/elevations can differ. Use one copy on all three rows.
    for (p, q) in a.cpts().iter().zip(b.cpts_mut()) {
        *q = q.scale(p.w / q.w);
        q.w = p.w;
    }
    Ok([a, b])
}

/// Re-express the same two homogeneous curves in one basis, without averaging nearby knots.
fn common_basis([mut a, mut b]: [Curve; 2]) -> Result<[Curve; 2]> {
    let degree = a.degree().max(b.degree());
    for c in [&mut a, &mut b] {
        c.knot_vector()?.validate(c.degree())?;
        if c.degree() < degree {
            *c = c.elevate_degree(degree - c.degree())?;
        }
    }
    let target = knot_union(a.knots(), b.knots());
    for c in [&mut a, &mut b] {
        let mut have = c.knots().iter().peekable();
        let missing: Vec<_> = target
            .iter()
            .copied()
            .filter(|t| {
                if have.peek().is_some_and(|v| **v == *t) {
                    have.next();
                    false
                } else {
                    true
                }
            })
            .collect();
        if !missing.is_empty() {
            *c = c.refine(&missing)?;
        }
        if c.knots() != target {
            return Err(invalid(
                "section refinement did not preserve the common knot vector",
            ));
        }
    }
    Ok([a, b])
}

/// Bound the length ratio of a forward chord parallel to `run`. In oblique
/// coordinates a chord `v` has components `(n·v / n·run, (run × v) / |run|²)`.
/// A chord `t*run`, `t > 0`, has norm `t` in these coordinates. The hull
/// gives a lower bound on this norm (max of the axial magnitude and the
/// transverse norm). Crown multiplies that bound by at most 4/3, below the
/// factor of two at which a quadratic would stall. This avoids retracing any forward
/// chord. A noncollinear chord cannot cancel the derivative; this is not a
/// certificate against reversed matching or intersections between run curves.
fn chord_scale(k: &Kernel, [a, b]: &[Curve; 2], run: Vec3, normal: Vec3) -> Result<f64> {
    let vector = |v: Vec3| [v.x, v.y, v.z].map(Ivl::from);
    let run = vector(run);
    let normal = vector(normal);
    let height = dot(normal, run);
    let length_sq = dot(run, run);
    // Bound subtraction error in Euclidean coordinates. The weights are shared,
    // so the difference curve is itself a rational spline on these same knots.
    let mut subtraction_error = 0.0_f64;
    let delta = a
        .cpts()
        .iter()
        .zip(b.cpts())
        .map(|(p, q)| {
            let [x, y, z] = [q.x - p.x, q.y - p.y, q.z - p.z];
            let e = next_up(x.abs().max(y.abs()).max(z.abs()) * f64::EPSILON + f64::from_bits(1));
            subtraction_error = subtraction_error.max(next_up(e / p.w));
            CPoint::new(x, y, z, p.w)
        })
        .collect();
    let chords = Curve::new(delta, a.degree(), a.knots().to_vec())?;
    let bound = |lo, hi| {
        let points = crv_piece_hull_points(&chords, &Extent1d::from_raw(lo, hi))?;
        points
            .into_iter()
            .map(|(p, err)| {
                let v = vector(p)
                    .map(|x| x + Ivl::new(-1.0, 1.0) * Ivl::from(next_up(err + subtraction_error)));
                let x = dot(normal, v) / height;
                let q = cross(run, v).map(|x| x / length_sq);
                (x, q)
            })
            .reduce(|(x, [a, b, c]), (y, [d, e, f])| (x.hull(y), [a.hull(d), b.hull(e), c.hull(f)]))
            .map(|(x, q)| x.mig().max(norm(q).lo))
    };
    let mut scale = 1.0_f64;
    // Stop when a hull separates from zero rather than solving for an exact
    // closest point. Bound subdivision even if the two rims actually touch.
    let mut todo: Vec<_> = a
        .knots()
        .windows(2)
        .filter_map(|pair| {
            let &[lo, hi] = pair else { return None };
            (hi > lo).then_some((lo, hi, 0u32))
        })
        .collect();
    while let Some((lo, hi, depth)) = todo.pop() {
        k.check_cancelled(OP)?;
        let lower = bound(lo, hi).filter(|x| x.is_finite()).unwrap_or(0.0);
        if lower > 0.0 {
            scale = scale.min(lower);
        } else if depth < 10 {
            let mid = lo + (hi - lo) * 0.5;
            todo.extend([(lo, mid, depth + 1), (mid, hi, depth + 1)]);
        } else {
            return Err(SolidError::NotConverged {
                operation: OP,
                iterations: depth,
            });
        }
    }
    Ok(scale)
}

/// With a common denominator the end derivative of a quadratic Bézier is
/// twice the adjacent Euclidean control difference. The two rims supply the
/// other rows, unchanged. This is the three-constraint Hermite interpolant.
fn patch([a, b]: [Curve; 2], end: DerivEnd, d: Vec3) -> Result<Net> {
    let (rim, offset) = if end == DerivEnd::Start {
        (&a, d * 0.5)
    } else {
        (&b, d * -0.5)
    };
    let middle = rim
        .cpts()
        .iter()
        .map(|p| {
            let mut mid = *p;
            mid.blend(p.w, CPoint::new(offset.x, offset.y, offset.z, 0.0));
            mid
        })
        .collect();
    let rows = vec![a.cpts().to_vec(), middle, b.cpts().to_vec()];
    // Positive coefficients bound the denominator away from zero on the patch.
    // The general skin solver's weight limits do not constrain this interpolation.
    if rows.iter().flatten().any(|p| {
        !(p.w > 0.0 && p.w.is_finite() && p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
    }) {
        return Err(invalid(
            "the loft control net is not finite with positive weights",
        ));
    }
    let net = Net::new(rows, 2, a.degree(), RUN.to_vec(), a.knots().to_vec())?;
    refine_control_net(net)
}

/// Sharply turning control polygons can misrepresent a regular span (e.g. a
/// tall parabola). Refine only transversely, where the source geometry may
/// turn sharply, without changing the surface or the quadratic run. Three
/// bisections per original span bound the extra controls; topology still
/// validates the result without adjusting its tolerances.
fn refine_control_net(mut net: Net) -> Result<Net> {
    for _ in 0..3 {
        let points: Vec<_> = net.rows().flatten().map(|p| p.to_euclid()).collect();
        if !grid_has_non_monotonic_points_to_rounding(&points, net.m() + 1, false, false)? {
            break;
        }
        let midpoints: Vec<_> = net
            .v_knots()
            .windows(2)
            .filter_map(|pair| {
                let &[a, b] = pair else { return None };
                (a < b).then_some(a + (b - a) * 0.5)
            })
            .collect();
        net = insert_knots(&net, &midpoints, Direction::V)?;
    }
    Ok(net)
}

/// One uncapped loft sheet, with each wall's original section indices.
pub struct PlanarG1Skin {
    /// The stitched wall faces, without end caps.
    pub brep: Brep,
    /// For each face, the original curve indices flattened in section order.
    pub sources: Vec<(FaceId, Vec<usize>)>,
}

/// Uncapped sheet through `sections`, exactly one tangent end, and its side
/// faces' input curve indices (flattened in section order).
///
/// # Errors
/// Invalid input for an invalid crown, open sections, a degenerate connector,
/// or a wall seam that does not close at the kernel's zone tolerance; otherwise
/// the section matcher, skin, or B-rep construction's error.
pub fn skin_planar_g1(
    k: &mut Kernel,
    sections: &[LoftSection; 2],
    tangent_at: usize,
    crown: f64,
    faced: bool,
    max_segments: usize,
) -> Result<PlanarG1Skin> {
    let end = match tangent_at {
        0 => DerivEnd::Start,
        1 => DerivEnd::End,
        _ => return Err(invalid("a two-section loft has only two ends")),
    };
    if !(-0.9..=3.0).contains(&crown) {
        return Err(invalid("a loft's crown must be between -0.9 and 3"));
    }
    // Express each input in a nearby sketch frame before computing moments or
    // matching. A tiny loop far from its sketch origin loses its area and
    // centre direction if the Green integrals subtract remote coordinates.
    let mut local = sections.clone();
    for section in &mut local {
        if let Some(seg) = section.segs.first() {
            let shift = seg.start();
            section.origin += section.x * shift.x + section.y * shift.y;
            for seg in &mut section.segs {
                seg.translate(-shift);
            }
        }
    }
    let sections = &local;
    let [first, last] = sections;
    let matched = match_loft_sections(sections, max_segments)?;
    let [a, b] = matched.as_slice() else {
        return Err(invalid("expected two matched sections"));
    };
    let normal = sections.each_ref().map(|s| s.x.cross(s.y));
    let offset = |s: &LoftSection| {
        let p = centroid(&s.segs);
        s.x * p.x + s.y * p.y
    };
    let run = (last.origin - first.origin) + (offset(last) - offset(first));
    let [first_normal, last_normal] = normal;
    let (active, active_normal) = if end == DerivEnd::Start {
        (a, first_normal)
    } else {
        (b, last_normal)
    };
    let height = run.dot(active_normal);
    // Treat a connector parallel to the rim plane to numerical precision as in-plane.
    // A smooth closed rim then has a tangent parallel to it somewhere, losing rank.
    if !height.is_finite() || height.abs() <= EFF_ZERO * run.length() * active_normal.length() {
        return Err(invalid(
            "the adjacent section's centre lies in the G1 section's plane: no transverse tangent",
        ));
    }
    let array = |v: Vec3| [v.x, v.y, v.z];
    let frame = |s: &LoftSection| Frame {
        origin: array(s.origin),
        x_axis: array(s.x),
        y_axis: array(s.y),
    };
    let [fa, fb] = [frame(first), frame(last)];
    let mut brep = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    let tol = brep.tolerance();
    let walks = [&a.segs, &b.segs];
    for (segs, s) in walks.into_iter().zip(sections) {
        let mut points = segs.iter().zip(segs.iter().cycle().skip(1));
        if segs.is_empty()
            || points.any(|(p, q)| {
                let gap = p.end() - q.start();
                let length = (s.x * gap.x + s.y * gap.y).length();
                !length.is_finite() || length > tol
            })
        {
            return Err(invalid("a loft section must be a closed loop"));
        }
    }
    // At the constrained rim, (D × tangent) · (tangent × normal) has
    // sign −D·normal for counter-clockwise winding. The matched walls wind
    // consistently, so choose the side once for the sheet, at that regular rim.
    let flip = if faced {
        (height > 0.0) == (signed_area(&active.segs) > 0.0)
    } else {
        // Matching winds the first loop counter-clockwise. The legacy
        // unfaced sheet used the input chain's original direction.
        signed_area(&first.segs) < 0.0
    };
    let mut walls = Vec::new();
    let mut scale = 1.0_f64;
    for (p, q) in a.segs.iter().zip(&b.segs) {
        k.check_cancelled(OP)?;
        let pair = compatible([section_curve(&fa, p)?, section_curve(&fb, q)?])?;
        scale = scale.min(chord_scale(k, &pair, run, active_normal)?);
        walls.push(pair);
    }
    // One speed for the entire loft keeps both sides of each seam on the same curve.
    let d = run * (scale * (1.0 + crown) / 3.0);
    let mut sources = Vec::new();
    for (pair, (left, right)) in walls.into_iter().zip(a.from.iter().zip(&b.from)) {
        k.check_cancelled(OP)?;
        let mut net = patch(pair, end, d)?;
        if flip {
            net.swap_uv();
        }
        let surface = Surface::new(SurfaceKind::BSpline(BSplineSurface::from_nurb(net, true)));
        let domain = surface.natural_uv_domain_if_built();
        let f = face_on(k, &mut brep, surface, &domain)?;
        let mut from: Vec<_> = left
            .iter()
            .copied()
            .chain(right.iter().map(|&j| first.segs.len() + j))
            .collect();
        from.sort_unstable();
        from.dedup();
        sources.push((f, from));
    }
    if sources.len() != a.segs.len() || sources.len() != b.segs.len() {
        return Err(invalid("the two sections do not have matching wall spans"));
    }
    let sewn = stitch_faces(k, &mut brep, tol, None, false)?;
    if usize::try_from(sewn.lamina_edges).ok() != sources.len().checked_mul(2) {
        return Err(invalid(
            "the loft's corresponding walls do not meet at the model tolerance",
        ));
    }
    brep.refresh_cached_topology();
    Ok(PlanarG1Skin { brep, sources })
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::math::Vec2;
    use crate::nurbs::crv_adv::p2::SumDiff;
    use crate::nurbs::knots::Side;
    use crate::nurbs::srf_basic::Direction;
    use crate::nurbs::srf_tool::p1::extract_iso_crv;

    fn frame(z: f64) -> Frame {
        Frame {
            origin: [0.0, 0.0, z],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        }
    }

    fn assert_rims(net: &Net, curves: &[Curve; 2]) {
        for (u, expected) in [0.0, 1.0].into_iter().zip(curves) {
            let rim = extract_iso_crv(net, u, Direction::V).unwrap();
            let difference = rim.sum_diff(expected, SumDiff::Minus).unwrap();
            // With positive weights the difference lies in its control hull, so this
            // bounds its distance from zero over the whole rim, not just at stations.
            for p in difference.cpts() {
                assert!(p.w.is_finite() && p.w > 0.0);
                assert!(p.to_euclid().length() < 1e-10, "rim difference: {p:?}");
            }
        }
    }

    #[test]
    fn a_polynomial_and_a_semicircle_make_a_smooth_wall_through_both_rims() {
        let arc = Seg2::Arc {
            center: Vec2::new(0.0, 0.0),
            radius: 8.0,
            start: -std::f64::consts::FRAC_PI_2,
            sweep: std::f64::consts::PI,
        };
        let line = Seg2::Line {
            a: Vec2::new(1.0, -8.0),
            b: Vec2::new(1.0, 8.0),
        };
        let a = section_curve(&frame(0.0), &line).unwrap();
        let b = section_curve(&frame(30.0), &arc).unwrap();
        assert_eq!(b.knots().iter().filter(|&&v| v == 0.5).count(), 2);
        let d = Vec3::new(0.0, 0.0, 10.0);
        for (curves, d) in [([a.clone(), b.clone()], d), ([b, a], -d)] {
            for (end, u, side) in [
                (DerivEnd::Start, 0.0, Side::Right),
                (DerivEnd::End, 1.0, Side::Left),
            ] {
                let net = patch(compatible(curves.clone()).unwrap(), end, d).unwrap();
                assert_rims(&net, &curves);
                let left = net
                    .derivs(0.5, 0.5, Side::Right, Side::Left, true, 1, 1)
                    .unwrap();
                let right = net
                    .derivs(0.5, 0.5, Side::Right, Side::Right, true, 1, 1)
                    .unwrap();
                // Each section is C1 at v = 0.5, so their Euclidean blend has equal
                // first derivatives there, a stronger claim than equal normals.
                assert!((left[0][1] - right[0][1]).length() < 1e-10);
                assert!((left[1][0] - right[1][0]).length() < 1e-10);
                for v in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    let at_end = net.derivs(u, v, side, Side::Left, true, 1, 1).unwrap();
                    assert!((at_end[1][0] - d).length() < 1e-10);
                }
            }
        }
    }

    #[test]
    fn chord_bound_subdivides_when_only_the_control_hull_contains_zero() {
        // The chord runs along (2t−1, 0, 4t(1−t)). Its unsplit control
        // hull contains zero on the base, but the chord itself never does.
        let make = |points| Curve::new(points, 2, RUN.to_vec()).unwrap();
        let a = make(vec![CPoint::new(0.0, 0.0, 0.0, 1.0); 3]);
        let b = make(vec![
            CPoint::new(-1.0, 0.0, 0.0, 1.0),
            CPoint::new(0.0, 0.0, 2.0, 1.0),
            CPoint::new(1.0, 0.0, 0.0, 1.0),
        ]);
        let mut k = Kernel::default();
        let l = chord_scale(&k, &[a.clone(), b], Vec3::Z, Vec3::Z).unwrap();
        // The true minimum in the max norm is (sqrt(5)−1)/2, where
        // |2t−1| = 4t(1−t). No positive value follows from the unsplit hull.
        assert!(l > 0.0 && l <= (5.0_f64.sqrt() - 1.0) / 2.0);
        let coincident = [a.clone(), a];
        assert!(matches!(
            chord_scale(&k, &coincident, Vec3::Z, Vec3::Z),
            Err(SolidError::NotConverged { .. })
        ));
        // Allow the original hull but stop before the first subdivided one.
        k.set_cancel_token(crate::CancelToken::after_polls(2));
        assert!(matches!(
            chord_scale(&k, &coincident, Vec3::Z, Vec3::Z),
            Err(SolidError::Cancelled { operation: OP })
        ));
        k.set_cancel_token(crate::CancelToken::with_fuel(1));
        assert!(matches!(
            chord_scale(&k, &coincident, Vec3::Z, Vec3::Z),
            Err(SolidError::OutOfFuel { operation: OP, .. })
        ));
    }

    #[test]
    fn arc_products_keep_distinct_span_knots() {
        let arc = |sweep| Seg2::Arc {
            center: Vec2::ZERO,
            radius: 8.0,
            start: 0.0,
            sweep,
        };
        let a = section_curve(&frame(0.0), &arc(std::f64::consts::PI)).unwrap();
        let b = section_curve(&frame(30.0), &arc(4.0 * std::f64::consts::PI / 3.0)).unwrap();
        let curves = [a, b];
        let net = patch(
            compatible(curves.clone()).unwrap(),
            DerivEnd::Start,
            Vec3::Z * 10.0,
        )
        .unwrap();
        assert_rims(&net, &curves);
        for v in [1.0 / 3.0, 0.5, 2.0 / 3.0] {
            assert!(net.v_knots().contains(&v));
            let left = net
                .derivs(0.5, v, Side::Right, Side::Left, true, 1, 1)
                .unwrap();
            let right = net
                .derivs(0.5, v, Side::Right, Side::Right, true, 1, 1)
                .unwrap();
            assert!((left[0][1] - right[0][1]).length() < 1e-10);
        }
    }

    #[test]
    fn unequal_conic_weights_are_preserved_in_the_quadratic_blend() {
        let conic = |w| Seg2::Conic {
            a: Vec2::new(-1.0, 0.0),
            apex: Vec2::new(0.0, 2.0),
            b: Vec2::new(1.0, 0.0),
            w,
        };
        // The first pair is unequal but within the NLib weight pruning tolerance.
        // The second has a product weight above the general skin's solver range.
        // The third is a valid sketch conic below that range, with no product needed.
        for (wa, wb) in [(0.999_8, 1.000_2), (99.0, 50.0), (5e-6, 5e-6)] {
            let curves = [
                section_curve(&frame(0.0), &conic(wa)).unwrap(),
                section_curve(&frame(30.0), &conic(wb)).unwrap(),
            ];
            let d = Vec3::new(0.0, 0.0, 10.0);
            let a = Vec3::new(0.0, 2.0 * wa / (1.0 + wa), 0.0);
            let b = Vec3::new(0.0, 2.0 * wb / (1.0 + wb), 30.0);
            for (end, mid) in [(DerivEnd::Start, a + d * 0.5), (DerivEnd::End, b - d * 0.5)] {
                let net = patch(compatible(curves.clone()).unwrap(), end, d).unwrap();
                assert_rims(&net, &curves);
                let want = a * 0.25 + mid * 0.5 + b * 0.25;
                let got = net.eval(0.5, 0.5, Side::Right, Side::Right).unwrap();
                assert!((got - want).length() < 1e-10, "{got:?} != {want:?}");
            }
        }
    }
}
