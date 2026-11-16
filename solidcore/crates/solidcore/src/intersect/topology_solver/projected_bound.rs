//! Whole-domain bounds for a projected point/surface maximum.
//!
//! Not upstream. Candidate generation is deliberately independent of this check:
//! every knot rectangle must fit inside the candidate point's own arithmetic
//! enclosure. Caller tolerance selects retained answers, not solve accuracy.
//! The global proof bounds a homogeneous Bernstein residual over every knot patch.
//! Deviations: no recursion; global certification supports degree at most 8 per
//! axis, 4,096 cells, and depth 64. Higher degrees explicitly refuse. Native
//! candidate construction uses at most 4096 UV seeds, each refined with at most
//! 48 active-set Newton steps. Seeds are proposals, never a completeness proof.

use crate::blend::fillet_section::certificate::{knot_cuts, nurbs_partials_over_piece};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::Nurbs;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::Vec2;
use crate::math::Vec3;

use super::projected_metric::{norm_squared, projected};

const OP: &str = "intersect::topology_solver::projected_bound";

fn refused(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

/// Certify the scalar maximum for the single-answer point/face operation.
/// A previous answer on another object supplies a feasible lower bound; a new
/// answer on this face must additionally pass the exact trim and point checks.
pub(super) fn certify<O: crate::geom::solution_array::SolutionObject>(
    k: &Kernel,
    objects: &dyn super::p1::TopologyObjects<O>,
    face: O,
    point: Vec3,
    solver: &crate::geom::global_solver::GlobalSolver<'_, O>,
) -> Result<()> {
    let (net, domain, normal, upper) = checked_candidate(k, objects, face, point, solver)?;
    if !maximum_is_bounded(k, &net, &domain, point, normal, upper)? {
        return Err(refused(
            "whole-face maximum does not fit the candidate point enclosure",
        ));
    }
    Ok(())
}

/// The candidate checks of [`certify`] without its whole-face bound, so tests
/// can tell a refused candidate from a refused bound.
#[cfg(test)]
pub(super) fn certify_candidate<O: crate::geom::solution_array::SolutionObject>(
    k: &Kernel,
    objects: &dyn super::p1::TopologyObjects<O>,
    face: O,
    point: Vec3,
    solver: &crate::geom::global_solver::GlobalSolver<'_, O>,
) -> Result<()> {
    checked_candidate(k, objects, face, point, solver).map(|_| ())
}

/// Construct feasible native UV proposals without solving temporary lifted
/// curves. Knot-cell and silhouette seeds are independently refined before
/// selection; only the unchanged whole-face certificate proves completeness.
pub(super) fn native_candidates<O: crate::geom::solution_array::SolutionObject>(
    k: &Kernel,
    objects: &dyn super::p1::TopologyObjects<O>,
    face: O,
    point: Vec3,
    vectors: &[Vec3],
    hints: &[crate::curve::curve::Curve],
) -> Result<Vec<(Vec2, f64)>> {
    let net = face_net(k, objects, face)?;
    let domain = objects.face_uv_domain(face)?;
    let normal = *vectors
        .first()
        .ok_or_else(|| refused("missing projection vector"))?;
    if ![
        point.x,
        point.y,
        point.z,
        normal.x,
        normal.y,
        normal.z,
        domain.min.x,
        domain.min.y,
        domain.max.x,
        domain.max.y,
    ]
    .iter()
    .all(|x| x.is_finite())
        || domain.min.x >= domain.max.x
        || domain.min.y >= domain.max.y
    {
        return Err(refused("native proposal domain or metric is not finite"));
    }
    let (brep, entity) = objects
        .face_entity(face)
        .ok_or_else(|| refused("missing face boundary"))?;
    if !is_exact_rectangle(k, brep, entity, &domain)? {
        return Err(refused(
            "native proposals require an exact rectangular trim",
        ));
    }
    let surface = objects.surface(objects.face_surface(face)?)?;
    native_uv_proposals(k, surface, &net, &domain, point, normal, hints)
}

/// Refine native proposals within a caller-proved rectangular face domain.
/// Returned values enclose their own native point; no global optimality is
/// implied until the independent whole-domain certificate succeeds.
pub(super) fn native_uv_proposals(
    k: &Kernel,
    surface: &crate::surface::surface::Surface,
    net: &Nurbs,
    domain: &Extent2d,
    point: Vec3,
    normal: Vec3,
    hints: &[crate::curve::curve::Curve],
) -> Result<Vec<(Vec2, f64)>> {
    const MAX_SEEDS: usize = 4096;
    // Count active cells before knot_cuts allocates the cut vectors. This is a
    // work bound, not an accuracy threshold or a surface-class special case.
    let active_spans = |knots: &[f64], lo: f64, hi: f64| -> Result<usize> {
        let mut count = 1_usize;
        let mut previous = None;
        for &knot in knots {
            k.check_cancelled(OP)?;
            if knot > lo && knot < hi && previous != Some(knot) {
                count = count
                    .checked_add(1)
                    .ok_or_else(|| refused("native UV proposal budget exceeded"))?;
                if count > MAX_SEEDS {
                    return Err(refused("native UV proposal budget exceeded"));
                }
            }
            previous = Some(knot);
        }
        Ok(count)
    };
    let u_count = active_spans(&net.u, domain.min.x, domain.max.x)?;
    let v_count = active_spans(&net.v, domain.min.y, domain.max.y)?;
    let cell_count = u_count
        .checked_mul(v_count)
        .filter(|&n| n <= MAX_SEEDS)
        .ok_or_else(|| refused("native UV proposal cell budget exceeded"))?;
    // The global proof supports 4096 cells. Proposals need not cover each one:
    // always retain the rectangle corners, then spread the bounded grid budget
    // across the whole cell list. Reserve half for trace hints when present.
    let hint_reserve = if hints.is_empty() { 0 } else { MAX_SEEDS / 2 };
    let selected_cells = cell_count.min((MAX_SEEDS - 4 - hint_reserve) / 9);
    let grid_count = selected_cells * 9 + 4;
    let us = knot_cuts(&net.u, net.p, net.nu, domain.min.x, domain.max.x, 0)?;
    let vs = knot_cuts(&net.v, net.q, net.nv, domain.min.y, domain.max.y, 0)?;
    let mut seeds = Vec::new();
    let mut append = |uv: Vec2| -> Result<()> {
        if !uv.x.is_finite() || !uv.y.is_finite() {
            return Err(refused("non-finite native UV seed"));
        }
        // A trace is only a hint. Projecting it into the already-proved rectangle
        // produces a feasible parameter, not a statement about the trace's error.
        let uv = Vec2::new(
            uv.x.max(domain.min.x).min(domain.max.x),
            uv.y.max(domain.min.y).min(domain.max.y),
        );
        if seeds.len() >= MAX_SEEDS {
            return Err(refused("native UV proposal budget exceeded"));
        }
        seeds.push(uv);
        Ok(())
    };
    for uv in [
        domain.min,
        Vec2::new(domain.min.x, domain.max.y),
        Vec2::new(domain.max.x, domain.min.y),
        domain.max,
    ] {
        append(uv)?;
    }
    for index in 0..selected_cells {
        k.check_cancelled(OP)?;
        let source = distributed_index(index, selected_cells, cell_count)?;
        let ui = source
            .checked_div(vs.len())
            .ok_or_else(|| refused("missing native v cells"))?;
        let vi = source
            .checked_rem(vs.len())
            .ok_or_else(|| refused("missing native v cells"))?;
        let &(u0, u1, _) = us.get(ui).ok_or_else(|| refused("missing native u cell"))?;
        let &(v0, v1, _) = vs.get(vi).ok_or_else(|| refused("missing native v cell"))?;
        for u in [u0, 0.5 * u0 + 0.5 * u1, u1] {
            for v in [v0, 0.5 * v0 + 0.5 * v1, v1] {
                append(Vec2::new(u, v))?;
            }
        }
    }
    // Trace density is chosen by its construction tolerance, not by this
    // optimization problem. A bounded set of parameter hints avoids making
    // dense traces a reason to reject an otherwise certifiable native maximum.
    // Endpoints and the midpoint are included; this is not a sampling proof.
    let remaining = MAX_SEEDS - grid_count;
    let hint_count = hints.len().min(remaining / 3);
    if hint_count > 0 {
        let per_hint = remaining
            .checked_div(hint_count)
            .ok_or_else(|| refused("native UV proposal budget exceeded"))?
            .min(17);
        // An even number of intervals includes both endpoints and the midpoint.
        let steps = i32::try_from(((per_hint - 1) / 2) * 2)
            .map_err(|_| refused("native UV proposal budget exceeded"))?;
        for index in 0..hint_count {
            k.check_cancelled(OP)?;
            // Cover the entire hint list even when it exceeds the budget.
            let source_index = distributed_index(index, hint_count, hints.len())?;
            let hint = hints
                .get(source_index)
                .ok_or_else(|| refused("missing native UV hint"))?;
            let interval = hint.natural_interval();
            if !interval.min().is_finite()
                || !interval.max().is_finite()
                || interval.min() > interval.max()
            {
                return Err(refused("non-finite native UV hint interval"));
            }
            for step in 0..=steps {
                let fraction = f64::from(step) / f64::from(steps);
                let t = (1.0 - fraction) * interval.min() + fraction * interval.max();
                let p = hint.evaluate_point(k, t)?;
                append(Vec2::new(p.x, p.y))?;
            }
        }
    }
    let mut proposals = Vec::new();
    for seed in seeds {
        k.check_cancelled(OP)?;
        let uv = refine_uv(k, surface, domain, point, normal, seed)?;
        let value = normal
            .cross(surface.evaluate_point(k, uv)? - point)
            .cross(normal)
            .length();
        let own = value_at(net, uv, point, normal)?;
        if !(domain.contains_point(uv, 0.0)
            && value.is_finite()
            && own.lo.is_finite()
            && own.hi.is_finite()
            && own.lo <= value
            && value <= own.hi)
        {
            return Err(refused(
                "native UV proposal does not fit its own point enclosure",
            ));
        }
        proposals.push((uv, value));
    }
    Ok(proposals)
}

/// Spread bounded proposals across a full ordered source list, including its
/// first and last elements whenever at least two proposals are available.
fn distributed_index(index: usize, count: usize, length: usize) -> Result<usize> {
    if count == 0 || count > length || index >= count {
        return Err(refused("invalid native proposal distribution"));
    }
    if count == 1 {
        return Ok(length / 2);
    }
    index
        .checked_mul(length - 1)
        .and_then(|n| n.checked_div(count - 1))
        .ok_or_else(|| refused("native UV proposal distribution overflow"))
}

/// Improve the chosen feasible face point on the native definition. This is
/// candidate construction only; the whole-domain certificate remains mandatory.
pub(super) fn refine_candidate<O: crate::geom::solution_array::SolutionObject>(
    k: &Kernel,
    objects: &dyn super::p1::TopologyObjects<O>,
    face: O,
    point: Vec3,
    solver: &mut crate::geom::global_solver::GlobalSolver<'_, O>,
) -> Result<()> {
    let Some(solution) = solver.solutions.get(0) else {
        return Ok(());
    };
    if !solution.objects.contains(&Some(face)) {
        return Ok(());
    }
    let (_, domain, normal, _) = checked_candidate(k, objects, face, point, solver)?;
    let surface = objects.surface(objects.face_surface(face)?)?;
    let uv = Vec2::new(solution.start.param(0)?, solution.start.param(1)?);
    let refined = refine_uv(k, surface, &domain, point, normal, uv)?;
    let value = normal
        .cross(surface.evaluate_point(k, refined)? - point)
        .cross(normal)
        .length();
    if value.is_finite() && value >= solution.start.solution_value {
        if let Some(solution) = solver.solutions.get_mut(0) {
            *solution.start.param_mut(0)? = refined.x;
            *solution.start.param_mut(1)? = refined.y;
            solution.start.solution_value = value;
        }
        solver.best_answer_so_far_sq = solver.best_answer_so_far_sq.max(value * value);
    }
    Ok(())
}

/// Bounded active-set Newton refinement of squared projected distance. A fixed
/// domain side releases only when the gradient points into the domain. No caller
/// retention tolerance is a convergence condition; non-improving/undefined steps
/// stop candidate refinement and leave acceptance to the independent certificate.
fn refine_uv(
    k: &Kernel,
    surface: &crate::surface::surface::Surface,
    domain: &Extent2d,
    point: Vec3,
    normal: Vec3,
    mut uv: Vec2,
) -> Result<Vec2> {
    use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    let project = |v: Vec3| normal.cross(v).cross(normal);
    let metric = |uv| -> Result<f64> {
        Ok(project(surface.evaluate_point(k, uv)? - point).length_squared())
    };
    let mut value = metric(uv)?;
    for _ in 0..48 {
        k.check_cancelled(OP)?;
        let d = surface.evaluate(
            k,
            uv,
            2,
            2,
            EvaluateFlags {
                non_zero_tangents: false,
                ..EvaluateFlags::default()
            },
        )?;
        let at = |u: usize, v: usize| {
            d.get(u)
                .and_then(|row| row.get(v))
                .copied()
                .ok_or_else(|| refused("missing native surface derivative"))
        };
        let q = project(at(0, 0)? - point);
        let du = project(at(1, 0)?);
        let dv = project(at(0, 1)?);
        let gu = q.dot(du);
        let gv = q.dot(dv);
        let huu = du.dot(du) + q.dot(project(at(2, 0)?));
        let huv = du.dot(dv) + q.dot(project(at(1, 1)?));
        let hvv = dv.dot(dv) + q.dot(project(at(0, 2)?));
        if ![gu, gv, huu, huv, hvv].iter().all(|x| x.is_finite()) {
            break;
        }
        let hold_u = (uv.x == domain.min.x && gu <= 0.0) || (uv.x == domain.max.x && gu >= 0.0);
        let hold_v = (uv.y == domain.min.y && gv <= 0.0) || (uv.y == domain.max.y && gv >= 0.0);
        let delta = match (hold_u, hold_v) {
            (true, false) if hvv < 0.0 => Vec2::new(0.0, -gv / hvv),
            (false, true) if huu < 0.0 => Vec2::new(-gu / huu, 0.0),
            (false, false) => {
                let determinant = huu * hvv - huv * huv;
                if !(huu < 0.0 && hvv < 0.0 && determinant > 0.0 && determinant.is_finite()) {
                    break;
                }
                Vec2::new(
                    (huv * gv - hvv * gu) / determinant,
                    (huv * gu - huu * gv) / determinant,
                )
            }
            _ => break,
        };
        let next = Vec2::new(
            (uv.x + delta.x).max(domain.min.x).min(domain.max.x),
            (uv.y + delta.y).max(domain.min.y).min(domain.max.y),
        );
        if ![next.x, next.y].iter().all(|x| x.is_finite()) || next == uv {
            break;
        }
        let next_value = metric(next)?;
        if !next_value.is_finite() || next_value < value {
            break;
        }
        uv = next;
        value = next_value;
    }
    Ok(uv)
}

fn face_net<O: crate::geom::solution_array::SolutionObject>(
    k: &Kernel,
    objects: &dyn super::p1::TopologyObjects<O>,
    face: O,
) -> Result<Nurbs> {
    use crate::blend::fillet_section::station::as_b_spline;
    use crate::surface::surface::SurfaceKind;
    let surface = objects.surface(objects.face_surface(face)?)?;
    if surface.procedural_blend().is_some() || surface.procedural_sweep().is_some() {
        return Err(refused(
            "projected maximum needs an enclosure of the surface definition",
        ));
    }
    match surface.kind_with_nurb(k)? {
        SurfaceKind::BSpline(_) => {}
        SurfaceKind::Plane(_)
        | SurfaceKind::Cone(_)
        | SurfaceKind::Sphere(_)
        | SurfaceKind::Torus(_) => {
            crate::surface::implicit::AnalyticSupport::check(k, surface)?;
        }
        _ => return Err(refused("projected maximum has no exact surface enclosure")),
    }
    let bs = as_b_spline(k, surface)?.ok_or_else(|| refused("missing surface net"))?;
    Nurbs::of(&bs)
}

fn checked_candidate<O: crate::geom::solution_array::SolutionObject>(
    k: &Kernel,
    objects: &dyn super::p1::TopologyObjects<O>,
    face: O,
    point: Vec3,
    solver: &crate::geom::global_solver::GlobalSolver<'_, O>,
) -> Result<(Nurbs, Extent2d, Vec3, f64)> {
    let net = face_net(k, objects, face)?;
    let domain = objects.face_uv_domain(face)?;
    let normal = *solver
        .opt_vectors
        .first()
        .ok_or_else(|| refused("missing projection vector"))?;
    let solution = solver
        .solutions
        .as_slice()
        .first()
        .ok_or_else(|| refused("projected maximum has no feasible candidate"))?;
    if solution.solution_type != crate::geom::solution_array::SolutionType::SingleValue {
        return Err(refused(
            "projected maximum requires a single feasible point",
        ));
    }
    let reported = solution.start.solution_value;
    if !reported.is_finite()
        || !solver.tolerance_3d.is_finite()
        || solver.tolerance_3d < 0.0
        || ![point.x, point.y, point.z, normal.x, normal.y, normal.z]
            .iter()
            .all(|x| x.is_finite())
    {
        return Err(refused("projected maximum has no finite error budget"));
    }
    // Enclose the actual returned object's point, even while certifying another
    // face. Never borrow the face under comparison's evaluation uncertainty.
    let upper;
    let candidate_face = solution
        .objects
        .iter()
        .flatten()
        .copied()
        .find(|object| object.kind() == crate::geom::solution_array::ObjectKind::Face);
    if let Some(candidate_face) = candidate_face {
        let candidate_net;
        let (own_net, own_domain) = if candidate_face == face {
            (&net, domain)
        } else {
            candidate_net = face_net(k, objects, candidate_face)?;
            (&candidate_net, objects.face_uv_domain(candidate_face)?)
        };
        let (b, f) = objects
            .face_entity(candidate_face)
            .ok_or_else(|| refused("missing face boundary"))?;
        if !is_exact_rectangle(k, b, f, &own_domain)? {
            return Err(refused(
                "candidate feasibility needs an exact rectangular trim boundary",
            ));
        }
        let uv = Vec2::new(solution.start.param(0)?, solution.start.param(1)?);
        if !uv.x.is_finite() || !uv.y.is_finite() || !own_domain.contains_point(uv, 0.0) {
            return Err(refused("projected candidate is outside the face domain"));
        }
        let value = value_at(own_net, uv, point, normal)?;
        if !(value.lo.is_finite()
            && value.hi.is_finite()
            && value.lo <= reported
            && reported <= value.hi)
        {
            return Err(refused(
                "projected candidate value is outside its own point enclosure",
            ));
        }
        upper = value.hi;
    } else if let Some(edge) = solution
        .objects
        .iter()
        .flatten()
        .copied()
        .find(|object| object.kind() == crate::geom::solution_array::ObjectKind::Edge)
    {
        let t = solution.start.param(0)?;
        let interval = objects.edge_interval(edge)?;
        if !t.is_finite() || t < interval.min() || t > interval.max() {
            return Err(refused(
                "returned edge parameter is outside its native interval",
            ));
        }
        let handle = objects
            .edge_curve(edge)?
            .ok_or_else(|| refused("returned edge has no curve definition"))?;
        let value = curve_point_enclosure(objects.curve(handle)?, t, point, normal)?;
        if !value.lo.is_finite()
            || !value.hi.is_finite()
            || reported < value.lo
            || reported > value.hi
        {
            return Err(refused(
                "returned edge value is outside its own point enclosure",
            ));
        }
        upper = value.hi;
    } else if solution.num_variables == 0 {
        // A point/vertex answer has no parameter uncertainty: its stored native
        // vertex coordinates define the point. A query vertex can also be named;
        // use only a point whose metric actually encloses the reported scalar.
        let mut vertex_upper = None;
        for object in solution.objects.iter().flatten() {
            if object.kind() != crate::geom::solution_array::ObjectKind::Vertex {
                continue;
            }
            let value = norm_squared(projected(
                relative(V3::from_vec3(objects.vertex_point(*object)?), point),
                V3::from_vec3(normal),
            ))
            .sqrt();
            if value.lo.is_finite()
                && value.hi.is_finite()
                && value.lo <= reported
                && reported <= value.hi
            {
                vertex_upper = Some(value.hi);
                break;
            }
        }
        upper = vertex_upper
            .ok_or_else(|| refused("returned vertex value is outside its own enclosure"))?;
    } else {
        return Err(refused("returned object has no native point enclosure"));
    }
    Ok((net, domain, normal, upper))
}

/// Enclose an actual native curve point; analytic approximations are not definitions.
fn curve_point_enclosure(
    curve: &crate::curve::curve::Curve,
    t: f64,
    point: Vec3,
    normal: Vec3,
) -> Result<Ivl> {
    use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
    use crate::curve::curve::CurveKind;
    if let Some(value) =
        crate::curve::native_affine::affine_projected_point_enclosure(curve, t, point, normal)
    {
        return Ok(value);
    }
    let CurveKind::BSpline(spline) = curve.kind() else {
        return Err(refused(
            "returned edge has no certified native curve definition",
        ));
    };
    let mut net = NurbsCurve::of(spline)?;
    let raw = spline
        .gw_nurb()
        .ok_or_else(|| refused("missing native curve net"))?;
    if raw.degree() != net.p || raw.cpts().len() != net.n || raw.knots() != net.u {
        return Err(refused(
            "native curve net does not match its point evaluator",
        ));
    }
    // NurbsCurve::of validates the representation but forms homogeneous points
    // through a rounded dehomogenize/rehomogenize round trip. Keep the original
    // defining coordinates here so those operations cannot move the point.
    net.pw.clear();
    for c in raw.cpts() {
        let w = if c.w == crate::nurbs::math::NOW {
            1.0
        } else {
            c.w
        };
        let z = if c.z == crate::nurbs::math::NOZ {
            0.0
        } else {
            c.z
        };
        if w <= 0.0 || ![c.x, c.y, z, w].iter().all(|x| x.is_finite()) {
            return Err(refused(
                "native curve needs finite coordinates and positive weights",
            ));
        }
        net.pw.extend([c.x, c.y, z, w]);
    }
    // None selects exactly the evaluator's native point span at an interior knot,
    // rather than hulling a different one-sided limit into arithmetic rounding.
    let derivatives = curve_derivs_over(&net, Ivl::from(t), 0, None)?;
    let position = *derivatives
        .first()
        .ok_or_else(|| refused("missing native curve point"))?;
    Ok(norm_squared(projected(relative(position, point), V3::from_vec3(normal))).sqrt())
}

fn relative(v: V3<Ivl>, point: Vec3) -> V3<Ivl> {
    V3 {
        x: v.x - Ivl::from(point.x),
        y: v.y - Ivl::from(point.y),
        z: v.z - Ivl::from(point.z),
    }
}

/// Enclose the metric at a feasible parameter, including arithmetic rounding.
pub(super) fn value_at(net: &Nurbs, uv: Vec2, point: Vec3, normal: Vec3) -> Result<Ivl> {
    // A point uses the native span (the right-hand piece at an interior knot,
    // the last piece at the upper natural endpoint). The generic box routine
    // hulls both sides: at a discontinuous knot that hull includes a geometric
    // jump, which is not arithmetic uncertainty of this returned point.
    let us = knot_cuts(&net.u, net.p, net.nu, uv.x, uv.x, 0)?;
    let vs = knot_cuts(&net.v, net.q, net.nv, uv.y, uv.y, 0)?;
    let ku = us
        .last()
        .ok_or_else(|| refused("missing candidate u span"))?
        .2;
    let kv = vs
        .last()
        .ok_or_else(|| refused("missing candidate v span"))?
        .2;
    let p = nurbs_partials_over_piece(net, ku, kv, uv.x, uv.x, uv.y, uv.y, 0)?;
    Ok(norm_squared(projected(
        relative(p.at(0, 0), point),
        V3::from_vec3(normal),
    ))
    .sqrt())
}

/// Check the stored boundary, without trusting the tolerance-based rectangular flag.
pub(super) fn is_exact_rectangle(
    k: &Kernel,
    b: &crate::topology::Brep,
    f: crate::topology::FaceId,
    domain: &Extent2d,
) -> Result<bool> {
    Ok(exact_rectangle_boundary(k, b, f, domain)?.is_some())
}

/// Return the exact stored boundary curves only after proving the rectangle.
pub(super) fn exact_rectangle_boundary(
    k: &Kernel,
    b: &crate::topology::Brep,
    f: crate::topology::FaceId,
    domain: &Extent2d,
) -> Result<Option<Vec<crate::curve::curve::Curve>>> {
    if !crate::topology::trimming_tools::face_uv_curves_present(b, f)? {
        return Ok(None);
    }
    let boundary = crate::topology::face::p1::create_oriented_trim_boundary(
        k,
        &mut b.clone_standing_for_self()?,
        f,
        true,
        false,
        None,
        &|entity| b.owner_identity(entity),
    )?;
    Ok(boundary_is_rectangle(&boundary, domain).then_some(boundary.curves))
}

fn boundary_is_rectangle(
    boundary: &crate::topology::face::p1::OrientedTrimBoundary,
    domain: &Extent2d,
) -> bool {
    if boundary.loops != [4] || boundary.curves.len() != 4 {
        return false;
    }
    let corners = [
        domain.min,
        Vec2::new(domain.max.x, domain.min.y),
        domain.max,
        Vec2::new(domain.min.x, domain.max.y),
    ];
    let mut edges = 0_u8;
    let mut first = None;
    let mut previous = None;
    for curve in &boundary.curves {
        let Some((a, b)) = rectangle_segment_ends(curve, &corners) else {
            return false;
        };
        let Some(i) = corners.iter().position(|p| *p == a) else {
            return false;
        };
        let Some(j) = corners.iter().position(|p| *p == b) else {
            return false;
        };
        let edge = if (i + 1) % 4 == j {
            i
        } else if (j + 1) % 4 == i {
            j
        } else {
            return false;
        };
        let bit = 1_u8 << edge;
        if edges & bit != 0 || previous.is_some_and(|p| p != a) {
            return false;
        }
        edges |= bit;
        first.get_or_insert(a);
        previous = Some(b);
    }
    edges == 15 && previous == first
}

/// Recognize affine trim definitions, rather than their sampled endpoints.
fn rectangle_segment_ends(
    curve: &crate::curve::curve::Curve,
    corners: &[Vec2; 4],
) -> Option<(Vec2, Vec2)> {
    use crate::curve::curve::CurveKind;
    use crate::curve::native_affine::exact_four_products_zero;
    use crate::nurbs::math::NOW;
    if let CurveKind::Line(line) = curve.kind() {
        let natural = line.natural_interval();
        let analytic = line.step_interval();
        if !line.is_bounded()
            || !natural.length().is_finite()
            || natural.length() <= 0.0
            || !analytic.length().is_finite()
            || analytic.length() <= 0.0
            || crate::geom::tol::is_zero(natural.length())
        {
            return None;
        }
        let p = line.line_point();
        let v = line.line_vector();
        let scale = line.line_scale();
        let component_matches = |origin, direction, parameter, target| {
            let scaled = scale * direction;
            exact_four_products_zero([
                (scale, direction, false),
                (scaled, 1.0, true),
                (0.0, 0.0, false),
                (0.0, 0.0, false),
            ]) && exact_four_products_zero([
                (origin, 1.0, false),
                (parameter, scaled, false),
                (target, 1.0, true),
                (0.0, 0.0, false),
            ])
        };
        let endpoint = |t| {
            corners.iter().copied().find(|c| {
                component_matches(p.x, v.x, t, c.x) && component_matches(p.y, v.y, t, c.y)
            })
        };
        let (a, b) = if line.inside_out() {
            (analytic.max(), analytic.min())
        } else {
            (analytic.min(), analytic.max())
        };
        return Some((endpoint(a)?, endpoint(b)?));
    }
    let CurveKind::BSpline(bs) = curve.kind() else {
        return None;
    };
    let net = bs.gw_nurb()?;
    let [a, b] = net.cpts() else { return None };
    let [t0, t1, t2, t3] = net.knots() else {
        return None;
    };
    if net.degree() != 1
        || t0 != t1
        || t2 != t3
        || t1 >= t2
        || curve.natural_interval().min() != *t1
        || curve.natural_interval().max() != *t2
        || ![a.w, b.w].iter().all(|w| *w == NOW || *w == 1.0)
    {
        return None;
    }
    Some((Vec2::new(a.x, a.y), Vec2::new(b.x, b.y)))
}

/// The whole-domain proof is independent of the returned point's arithmetic.
pub(super) use super::projected_residual::maximum_is_bounded;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_native_proposals_cannot_certify_a_missed_peak() {
        use crate::curve::curve::Curve;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as NativeSurface;
        use crate::surface::surface::Surface;
        let k = Kernel::default();
        // x(u)=120*u^2*(1-u)^2*(u-.5)^2. Every coarse grid and hint
        // seed is zero with zero gradient, but x(.25)=135/512 > 0.
        let rows = [0.0, 0.0, 2.0, -3.0, 2.0, 0.0, 0.0]
            .into_iter()
            .map(|x| vec![CPoint::new(x, 0.0, 0.0, 1.0); 2])
            .collect();
        let spline = BSplineSurface::from_nurb(
            NativeSurface::new(
                rows,
                6,
                1,
                [vec![0.0; 7], vec![1.0; 7]].concat(),
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap(),
            false,
        );
        let net = Nurbs::of(&spline).unwrap();
        let surface = Surface::from(spline);
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let hint = Curve::from(
            BSplineCurve::create_line_segment(
                2,
                Vec3::new(0.5, 0.0, 0.0),
                Vec3::new(0.5, 1.0, 0.0),
            )
            .unwrap(),
        );
        let proposals =
            native_uv_proposals(&k, &surface, &net, &domain, Vec3::ZERO, Vec3::Z, &[hint]).unwrap();
        assert!(!proposals.is_empty());
        let witness = surface.evaluate_point(&k, Vec2::new(0.25, 0.5)).unwrap().x;
        assert_eq!(witness, 135.0 / 512.0);
        for (uv, _) in proposals {
            let own = value_at(&net, uv, Vec3::ZERO, Vec3::Z).unwrap();
            assert!(own.hi < witness);
            assert!(!maximum_is_bounded(&k, &net, &domain, Vec3::ZERO, Vec3::Z, own.hi).unwrap());
        }
    }

    #[test]
    fn bounded_proposals_preserve_a_512_cell_affine_definition() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as NativeSurface;
        use crate::surface::surface::Surface;
        let rows = (0..=32)
            .map(|x| {
                (0..=16)
                    .map(|y| CPoint::new(f64::from(x), f64::from(y), 0.0, 1.0))
                    .collect()
            })
            .collect();
        let knots = |n| {
            std::iter::once(0.0)
                .chain((0..=n).map(f64::from))
                .chain(std::iter::once(f64::from(n)))
                .collect()
        };
        let spline = BSplineSurface::from_nurb(
            NativeSurface::new(rows, 1, 1, knots(32), knots(16)).unwrap(),
            false,
        );
        let net = Nurbs::of(&spline).unwrap();
        let surface = Surface::from(spline);
        let k = Kernel::default();
        let domain = Extent2d::from_bounds(0.0, 0.0, 32.0, 16.0);
        let proposals =
            native_uv_proposals(&k, &surface, &net, &domain, Vec3::ZERO, Vec3::Z, &[]).unwrap();
        assert!(proposals.len() <= 4096);
        let &(uv, value) = proposals.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        assert_eq!(uv, Vec2::new(32.0, 16.0));
        assert_eq!(value, 1280.0_f64.sqrt());
        let own = value_at(&net, uv, Vec3::ZERO, Vec3::Z).unwrap();
        assert!(maximum_is_bounded(&k, &net, &domain, Vec3::ZERO, Vec3::Z, own.hi).unwrap());
    }

    fn hidden_peak() -> Nurbs {
        // S(u,v)=(4u(1-u),4v(1-v),0): all four corners project to
        // the origin, but the interior reaches (1,1,0).
        let mut pw = Vec::new();
        for x in [0.0, 2.0, 0.0] {
            for y in [0.0, 2.0, 0.0] {
                pw.extend([x, y, 0.0, 1.0]);
            }
        }
        Nurbs {
            p: 2,
            q: 2,
            nu: 3,
            nv: 3,
            u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            pw,
        }
    }

    #[test]
    fn upper_domain_jump_is_not_omitted_from_whole_face_proof() {
        let mut pw = Vec::new();
        for x in [0.0, 1.0, 100.0, 100.0] {
            for _ in 0..2 {
                pw.extend([x, 0.0, 0.0, 1.0]);
            }
        }
        let net = Nurbs {
            p: 1,
            q: 1,
            nu: 4,
            nv: 2,
            u: vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0],
            v: vec![0.0, 0.0, 1.0, 1.0],
            pw,
        };
        let normal = Vec3::new(0.0, 0.0, 1.0);
        let endpoint = value_at(&net, Vec2::new(0.5, 0.5), Vec3::ZERO, normal).unwrap();
        assert!(endpoint.lo > 2.0);
        // The left patch's closed limit is only 1, but u=.5 itself is 100.
        assert!(!maximum_is_bounded(
            &Kernel::default(),
            &net,
            &Extent2d::from_bounds(0.0, 0.0, 0.5, 1.0),
            Vec3::ZERO,
            normal,
            2.0
        )
        .unwrap());
    }

    #[test]
    fn curved_native_edge_point_uses_only_its_default_knot_side() {
        use crate::curve::curve::Curve;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NativeCurve;
        use crate::nurbs::math::CPoint;
        let controls = [100.0, 100.0, 100.0, 0.0, 0.2, 1.0]
            .map(|x| CPoint::new(x, 0.0, 0.0, 1.0))
            .to_vec();
        let curve = Curve::from(BSplineCurve::from_nurb(
            NativeCurve::new(
                controls,
                2,
                vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0],
            )
            .unwrap(),
            3,
            false,
        ));
        let normal = Vec3::new(0.0, 0.0, 1.0);
        let point = curve_point_enclosure(&curve, 0.5, Vec3::ZERO, normal).unwrap();
        assert!(point.lo <= 0.0 && point.hi < 1.0);
        let end = curve_point_enclosure(&curve, 1.0, Vec3::ZERO, normal).unwrap();
        assert!(end.lo <= 1.0 && end.hi >= 1.0 && end.hi < 2.0);
    }

    #[test]
    fn projected_native_refinement_removes_small_normal_candidate_deficit() {
        use crate::surface::sphere::{Sphere, SphereOptions};
        use crate::surface::surface::Surface;
        let k = Kernel::default();
        let mut sphere = Sphere::new(
            &k,
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Extent2d::from_bounds(0.0, -60.0, 360.0, 60.0),
            0.5,
            SphereOptions::default(),
        )
        .unwrap();
        sphere.make_nurb(&k).unwrap();
        let surface = Surface::from(sphere);
        let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let angle = 45.0_f64.to_radians();
        let normal = Vec3::new(angle.sin(), 0.0, angle.cos()) * 0.001;
        let point = Vec3::new(0.2, 0.3, 2.0);
        let expected = 0.5 * normal.length_squared()
            + normal
                .cross(point - Vec3::new(0.0, 0.0, -1.0))
                .cross(normal)
                .length();
        let net = Nurbs::of(
            &crate::blend::fillet_section::station::as_b_spline(&k, &surface)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        // Actual prior PointFace candidate, approximately 110 own-enclosure
        // widths below the closed-form projected sphere maximum.
        let before = Vec2::new(0.9641190747292663, 0.13249057300975678);
        assert!(value_at(&net, before, point, normal).unwrap().hi < expected);
        let after = refine_uv(&k, &surface, &domain, point, normal, before).unwrap();
        assert!(domain.contains_point(after, 0.0));
        let bound = value_at(&net, after, point, normal).unwrap();
        assert!(
            bound.lo <= expected && expected <= bound.hi,
            "refined={after:?}, own={bound:?}, expected={expected:.17e}"
        );
    }

    #[test]
    fn point_enclosure_excludes_an_unrelated_discontinuous_knot_limit() {
        // Left patch is x=100; native right patch starts at x=0 and grows to 1.
        // A box hull at u=.5 would incorrectly grant the returned point 100
        // units of "rounding" and certify it over the whole right patch.
        let mut pw = Vec::new();
        for x in [100.0, 100.0, 0.0, 1.0] {
            for _ in 0..2 {
                pw.extend([x, 0.0, 0.0, 1.0]);
            }
        }
        let net = Nurbs {
            p: 1,
            q: 1,
            nu: 4,
            nv: 2,
            u: vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0],
            v: vec![0.0, 0.0, 1.0, 1.0],
            pw,
        };
        let normal = Vec3::new(0.0, 0.0, 1.0);
        let value = value_at(&net, Vec2::new(0.5, 0.5), Vec3::ZERO, normal).unwrap();
        assert!(value.lo <= 0.0 && value.hi < 1.0);
        assert!(!maximum_is_bounded(
            &Kernel::default(),
            &net,
            &Extent2d::from_bounds(0.5, 0.0, 1.0, 1.0),
            Vec3::ZERO,
            normal,
            value.hi,
        )
        .unwrap());
        let end = value_at(&net, Vec2::new(1.0, 0.5), Vec3::ZERO, normal).unwrap();
        assert!(end.lo <= 1.0 && end.hi >= 1.0 && end.hi < 2.0);
    }

    #[test]
    fn maximum_uses_only_candidate_point_enclosure() {
        let k = Kernel::default();
        let net = hidden_peak();
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let normal = Vec3::new(0.0, 0.0, 1.0);
        let value = value_at(&net, Vec2::new(0.5, 0.5), Vec3::ZERO, normal).unwrap();
        let upper = value.hi;
        assert!(maximum_is_bounded(&k, &net, &domain, Vec3::ZERO, normal, upper,).unwrap());
        assert!(!maximum_is_bounded(&k, &net, &domain, Vec3::ZERO, normal, 1.0,).unwrap());
    }

    #[test]
    fn analytic_trim_proof_uses_exact_members_not_a_stale_net_or_rounded_endpoints() {
        use crate::curve::curve::Curve;
        use crate::curve::line::Line;
        use crate::geom::extent1d::Extent1d;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        let y = 1e16;
        let corners = [
            Vec2::new(0.0, y),
            Vec2::new(1.0, y),
            Vec2::new(1.0, y + 2.0),
            Vec2::new(0.0, y + 2.0),
        ];
        let base =
            BSplineCurve::create_line_segment(2, Vec3::new(0.0, y, 0.0), Vec3::new(1.0, y, 0.0))
                .unwrap();
        let line = |dy| {
            Curve::from(Line::from_parts(
                base.clone(),
                Vec3::new(0.0, y, 0.0),
                Vec3::new(1.0, dy, 0.0),
                Extent1d::from_min_max(0.0, 1.0),
                1.0,
                false,
            ))
        };
        assert_eq!(
            rectangle_segment_ends(&line(0.0), &corners),
            Some((corners[0], corners[1]))
        );
        let tilted = line(0.25);
        assert_eq!(tilted.evaluate_point(&Kernel::default(), 1.0).unwrap().y, y);
        assert!(rectangle_segment_ends(&tilted, &corners).is_none());
    }

    #[test]
    fn almost_rectangular_trims_do_not_prove_feasibility() {
        use crate::curve::curve::Curve;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::topology::face::p1::OrientedTrimBoundary;
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let boundary = |inset| {
            let corners = [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(1.0 - inset, 1.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ];
            OrientedTrimBoundary {
                loops: vec![4],
                curves: corners
                    .iter()
                    .zip(corners.iter().cycle().skip(1))
                    .map(|(&a, &b)| {
                        Curve::from(BSplineCurve::create_line_segment(2, a, b).unwrap())
                    })
                    .collect(),
            }
        };
        assert!(boundary_is_rectangle(&boundary(0.0), &domain));
        assert!(!boundary_is_rectangle(&boundary(1e-12), &domain));
        let mut disconnected = boundary(0.0);
        disconnected.curves.swap(1, 2);
        assert!(!boundary_is_rectangle(&disconnected, &domain));
        let mut duplicate = boundary(0.0);
        duplicate.curves[3] = duplicate.curves[2].clone();
        assert!(!boundary_is_rectangle(&duplicate, &domain));
    }

    #[test]
    fn a_missed_interior_maximum_cannot_be_certified() {
        let k = Kernel::default();
        let net = hidden_peak();
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let normal = Vec3::new(0.0, 0.0, 1.0);
        assert!(!maximum_is_bounded(&k, &net, &domain, Vec3::ZERO, normal, 1.0).unwrap());
        assert!(
            maximum_is_bounded(&k, &net, &domain, Vec3::ZERO, normal, 2.0_f64.sqrt() + 1e-6,)
                .unwrap()
        );
    }

    #[test]
    fn non_unit_projection_is_bounded_in_the_requested_metric() {
        let k = Kernel::default();
        let net = hidden_peak();
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let normal = Vec3::new(0.0, 0.0, 2.0);
        assert!(!maximum_is_bounded(&k, &net, &domain, Vec3::ZERO, normal, 2.0).unwrap());
        assert!(maximum_is_bounded(
            &k,
            &net,
            &domain,
            Vec3::ZERO,
            normal,
            4.0 * 2.0_f64.sqrt() + 1e-6,
        )
        .unwrap());
    }

    #[test]
    fn exact_zero_projection_succeeds_but_invalid_bounds_refuse() {
        let k = Kernel::default();
        let net = hidden_peak();
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        // N=0 makes every homogeneous projected numerator identically zero.
        // Unlike the former mean-value arithmetic floor, the residual proves
        // this exact identity without adding any candidate or caller allowance.
        assert!(maximum_is_bounded(&k, &net, &domain, Vec3::ZERO, Vec3::ZERO, 0.0).unwrap());
        for invalid_upper in [-1.0, f64::NAN, f64::INFINITY] {
            assert!(
                !maximum_is_bounded(&k, &net, &domain, Vec3::ZERO, Vec3::ZERO, invalid_upper)
                    .unwrap()
            );
        }
        assert!(!maximum_is_bounded(
            &k,
            &net,
            &domain,
            Vec3::new(f64::NAN, 0.0, 0.0),
            Vec3::ZERO,
            0.0
        )
        .unwrap());
        let invalid = Extent2d::from_bounds(0.0, 0.0, f64::INFINITY, 1.0);
        assert!(!maximum_is_bounded(
            &k,
            &net,
            &invalid,
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 1.0),
            2.0,
        )
        .unwrap());
    }

    #[test]
    fn cancellation_is_not_reported_as_an_unproved_bound() {
        let token = crate::kernel::CancelToken::default();
        token.cancel();
        let k = Kernel {
            cancel: Some(token),
            ..Kernel::default()
        };
        let result = maximum_is_bounded(
            &k,
            &hidden_peak(),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 1.0),
            2.0,
        );
        assert!(matches!(result, Err(SolidError::Cancelled { .. })));
    }
}
