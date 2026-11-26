//! A live fillet solver's trace reach: how far its base surfaces' domains are extended
//! across the filleted edge, so that the trace reaches as far as its rails can lie.
//!
//! Upstream: not in SMLib at the pinned commit. SolidCraft's fork (tangency-labs/smlib,
//! rev `68fb9162`) adds the static `sm_ExtendTraceToItsRails` (`SmFilletExecutive.cpp`)
//! in commit `a9277fd` (WR-609) and calls it for every solver before the corners'
//! vertices in commit `6e77631` (WR-610). It runs only with the opt-in
//! `Kernel::fixes.fillet_trace_reaches_rails`.
//!
//! | Rust | Upstream (fork) |
//! |------|-----------------|
//! | `extend_trace_to_its_rails` (crate) | `sm_ExtendTraceToItsRails` |
//!
//! Deviations: the variable-radius cast (`SM_CAST_PTR(SmVariableRadiusFS, pFS)`) is a
//! match on the session's concrete solver. A failed law evaluation in
//! `SmVariableRadiusFS::GetFilletRadius` is returned as its error, where the fork's `SER`
//! inside that `double` function yields its status code (1001, 1007) as the radius
//! (#21908); a station where the law's continuation gives no usable radius
//! (`RoundRefusal::LawRadiusUnusable`, solidcore's, #2706) sets no reach.
//! A failed `GetOrCreateUVTrimCurve` skips that surface's across test, as the fork
//! does, but an `Unported` one propagates. There is no recursion and no unbounded
//! loop (7 and 65 fixed samples).

use crate::blend::fillet_executive::session::SolverObject;
use crate::blend::round_room::RoundRefusal;
use crate::error::blend::BlendFailure;
use crate::error::{EntityRef, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO_SQRT, PI};
use crate::nurbs::knots::Side;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::edgeuse::p3::get_or_create_uv_trim_curve;
use crate::topology::{edge, vertex, Brep, EdgeId, FaceId, VertexId};

const OP: &str = "blend::fillet_executive::session::trace_reach::extend_trace_to_its_rails";

/// `smos_Max` of two doubles.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `smos_Min` of two doubles.
fn smos_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// Unit tangent of `edge_id` leaving `vertex_id`, or `None` when `edge_id` is not
/// incident on `vertex_id` or its end tangent cannot be evaluated or unitized.
fn leaving_tangent(
    k: &Kernel,
    target: &Brep,
    edge_id: EdgeId,
    vertex_id: VertexId,
) -> Result<Option<Vec3>> {
    let (start_v, end_v) = edge::start_end_vertices(target, edge_id)?;
    let at_start = if start_v == Some(vertex_id) {
        true
    } else if end_v == Some(vertex_id) {
        false
    } else {
        return Ok(None);
    };
    let raw = match edge::end_tangent(k, target, edge_id, at_start) {
        Ok(t) => t,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(None),
    };
    let dir = if at_start { raw } else { -raw };
    match dir.unitize() {
        Ok(unit) => Ok(Some(unit)),
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => Ok(None),
    }
}

/// How far past an end vertex a rail can project along `edge_id` when a side edge of
/// one of `edge_id`'s faces leaves that vertex obliquely backward (past the end of
/// `edge_id` at an obtuse interior corner). Returns `0.0` when no side edge runs
/// backward past either end vertex (orthogonal or acute corners, or a closed edge).
fn oblique_end_reach(
    k: &Kernel,
    target: &Brep,
    edge_id: EdgeId,
    edge_faces: [Option<FaceId>; 2],
    d_rail: f64,
    radius: f64,
) -> Result<f64> {
    if edge::is_closed(target, edge_id)? {
        return Ok(0.0);
    }
    let (Some(start_v), Some(end_v)) = edge::start_end_vertices(target, edge_id)? else {
        return Ok(0.0);
    };
    if start_v == end_v {
        return Ok(0.0);
    }
    let [face0, face1] = edge_faces;
    let min_sin = (PI / 180.0).sin();
    let mut max_along = 0.0_f64;
    for v in [start_v, end_v] {
        let Some(t_fil) = leaving_tangent(k, target, edge_id, v)? else {
            continue;
        };
        for side in vertex::edges(target, v, None)? {
            if side == edge_id || edge::is_closed(target, side)? {
                continue;
            }
            let shares_face = edge::is_connected_to_face(target, side, face0)?
                || edge::is_connected_to_face(target, side, face1)?;
            if !shares_face {
                continue;
            }
            let Some(t_side) = leaving_tangent(k, target, side, v)? else {
                continue;
            };
            let cos_back = t_side.dot(-t_fil);
            let sin_theta = t_side.cross(t_fil).length();
            if cos_back > 0.0 && sin_theta > min_sin {
                max_along = smos_max(max_along, d_rail * (cos_back / sin_theta));
            }
        }
    }
    Ok(if max_along > 0.0 {
        2.0 * max_along + radius
    } else {
        0.0
    })
}

/// `sm_ExtendTraceToItsRails(pFS)` (fork, WR-609/WR-610): sets the solver's trace reach
/// ([`crate::blend::fillet_solver::p1::FilletSolver::set_trace_reach`]) when the default
/// extension does not reach as far as its rails can lie from the filleted edge.
///
/// A rail lies `r / tan(t/2)` from the edge, `r` the radius (or offset) and `t` the
/// smallest angle between the two faces' planes along the edge (seven samples, at
/// least one degree): the base domains are made to reach twice that and `r` more, `r`
/// the largest over the trace's whole length (a variable round's law as it runs past
/// the edge's ends, 65 samples), in the parameter directions that run across the edge
/// (not mostly along it, by the edge's UV direction at its middle scaled to 3D). A
/// solver the default reach covers is left as it is. `target` holds the solver's edge
/// uses; their UV trim curves are made when missing.
///
/// # Errors
///
/// A [`BlendFailure::Defect`] (`SM_ERR_NULL_POINTER`) for a solver without its first edge
/// use or its edge, [`BlendFailure::EdgeWithoutCurve`] for an edge without its curve; those
/// of the curve and surface evaluations and the drops.
pub(crate) fn extend_trace_to_its_rails(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
) -> Result<()> {
    let base = solver.base();
    let (Some(first), Some(second)) = (base.surface(0)?, base.surface(1)?) else {
        return Ok(());
    };
    let offsets = [first.surface.clone(), second.surface.clone()];
    let bases: [&Surface; 2] = [offsets[0].base_surface(), offsets[1].base_surface()];
    let null_defect = |why| {
        Failure::new(OP, BlendFailure::Defect { why })
            .with_status(SM_ERR_NULL_POINTER)
            .into_error()
    };
    let edgeuse = base.edgeuse(0)?.ok_or_else(|| {
        null_defect(
            "a round's solver with both offset surfaces has no first edge use: the solver is \
             set up on the filleted edge's edge uses",
        )
    })?;
    let edge_id = eu::edge(target, edgeuse)?.ok_or_else(|| {
        null_defect(
            "a round's solver's first edge use has no edge: the solver was set up on that edge \
             use of the filleted edge",
        )
    })?;
    let curve = edge::curve(target, edge_id)?
        .ok_or_else(|| {
            SolidError::blend_at(OP, BlendFailure::EdgeWithoutCurve, EntityRef::Edge(edge_id))
        })?
        .clone();
    let interval = edge::interval(target, edge_id)?;

    // the smallest angle between the faces' planes along the edge
    let mut angle = PI / 2.0;
    for i in 1..8 {
        let pnt = curve.evaluate_point(k, interval.evaluate(f64::from(i) / 8.0))?;
        let mut normals = [Vec3::UNDEF; 2];
        let mut both = true;
        for (normal, surface) in normals.iter_mut().zip(bases) {
            if !both {
                break;
            }
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            let drop =
                surface.drop_point::<NoObject>(k, pnt, &domain, &DropPointOptions::default())?;
            both = drop.success;
            if drop.success {
                *normal = surface.evaluate_normal(k, drop.result_uv, Side::Left, Side::Left)?;
            }
        }
        if !both {
            continue;
        }
        let [n0, n1] = normals;
        let cos = smos_min(1.0, n0.dot(n1).abs());
        angle = smos_min(angle, cos.acos());
    }
    angle = smos_max(angle, PI / 180.0);

    // the largest radius over the trace's length: the edge, and for a variable round (its
    // law growing on) as far past the ends as SmVariableRadiusFS::LoadInitialValues lets it run
    let mut radius = smos_max(
        offsets[0].offset_distance().abs(),
        offsets[1].offset_distance().abs(),
    );
    let factor = base.extension_factor();
    let edgeuses = [base.edgeuse(0)?, base.edgeuse(1)?];
    if let SolverObject::VariableRadius(vr) = &mut *solver {
        let law_ivl = vr.fillet_law_interval();
        let past = 5.0 * factor * law_ivl.length();
        let traced = Extent1d::from_min_max(law_ivl.min() - past, law_ivl.max() + past);
        for i in 0..=64 {
            let t = traced.evaluate(f64::from(i) / 64.0);
            // SmVariableRadiusFS::GetFilletRadius; a failed evaluation is its error, not
            // its status number as a radius (#21908)
            let r = match vr.law.evaluate(t, &law_ivl, vr.law_orientation) {
                Ok(values) => values.first().copied().unwrap_or(0.0),
                // a station past the law's ends where its continuation gives no usable
                // radius (#2706): the trace does not run there, so it sets no reach
                Err(SolidError::RoundRefused {
                    refusal: RoundRefusal::LawRadiusUnusable { .. },
                    ..
                }) => continue,
                Err(e) => return Err(e),
            };
            radius = smos_max(radius, r.abs());
        }
    }
    // `!( dRadius > 0.0 )`: a NaN radius returns too
    let positive = radius > 0.0;
    if !positive {
        return Ok(());
    }
    let reach = (2.0 / (angle / 2.0).tan() + 1.0) * radius;
    let [eu0, eu1] = edgeuses;
    let edge_faces = [
        match eu0 {
            Some(id) => eu::face(target, id)?,
            None => None,
        },
        match eu1 {
            Some(id) => eu::face(target, id)?,
            None => None,
        },
    ];
    let along_reach = oblique_end_reach(
        k,
        target,
        edge_id,
        edge_faces,
        radius / (angle / 2.0).tan(),
        radius,
    )?;

    // Each base surface's parameter directions that run across or along the edge (by the
    // edge's direction on it in 3D proportions), and whether the default reaches.
    let mut across = [[true, true], [true, true]];
    let mut along_dirs = [[true, true], [true, true]];
    let mut d3d_all = [[0.0_f64; 2]; 2];
    let mut short = false;
    for ((((surface, row), along_row), d3d_slot), edgeuse) in bases
        .into_iter()
        .zip(across.iter_mut())
        .zip(along_dirs.iter_mut())
        .zip(d3d_all.iter_mut())
        .zip(edgeuses)
    {
        let domain: Extent2d = surface.natural_uv_domain_with_nurb(k)?;
        let mid = domain.mid();
        let ends: [Point3; 4] = [
            surface.evaluate_point(k, Point2::new(domain.min.x, mid.y))?,
            surface.evaluate_point(k, Point2::new(domain.max.x, mid.y))?,
            surface.evaluate_point(k, Point2::new(mid.x, domain.min.y))?,
            surface.evaluate_point(k, Point2::new(mid.x, domain.max.y))?,
        ];
        let [u_min, u_max, v_min, v_max] = ends;
        let d3d = [u_min.distance(u_max), v_min.distance(v_max)];
        *d3d_slot = d3d;
        let uv_size = domain.size();
        for d in d3d {
            if d > EFF_ZERO_SQRT && factor * d < reach {
                short = true;
            }
        }
        let Some(edgeuse) = edgeuse else {
            continue;
        };
        match get_or_create_uv_trim_curve(k, target, edgeuse, false, false, true) {
            Ok(fetch) => match fetch.status {
                Ok(()) => {}
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => continue,
            },
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => continue,
        }
        let Some(uv) = eu::uv_trim_curve(target, edgeuse)?.0 else {
            continue;
        };
        let derivs = uv.evaluate(k, uv.natural_interval().evaluate(0.5), 1, true, false)?;
        let Some(&tangent) = derivs.get(1) else {
            continue;
        };
        let scale = |size: f64, d: f64| if size > 0.0 { d / size } else { 0.0 };
        let along = [
            tangent.x * scale(uv_size.x, d3d[0]),
            tangent.y * scale(uv_size.y, d3d[1]),
        ];
        let len = (along[0] * along[0] + along[1] * along[1]).sqrt();
        if len > 0.0 {
            for ((flag, along_flag), a) in row.iter_mut().zip(along_row.iter_mut()).zip(along) {
                let ratio = a.abs() / len;
                *flag = ratio < 0.9;
                *along_flag = ratio > 0.1;
            }
        }
    }
    if short {
        solver.base_mut().set_trace_reach(reach, across);
    }
    if along_reach > 0.0 {
        let along_short = d3d_all.into_iter().zip(along_dirs).any(|(d_pair, flags)| {
            d_pair
                .into_iter()
                .zip(flags)
                .any(|(d, is_along)| is_along && d > EFF_ZERO_SQRT && factor * d < along_reach)
        });
        if along_short {
            solver
                .base_mut()
                .set_trace_reach_along(along_reach, along_dirs);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::fillet_executive::session::{FilletObject, FilletSession};
    use crate::blend::fillet_executive::{FilletLaw, FilletLawKind};
    use crate::primitive::api_primitives::create_box;

    /// #21908: a law evaluation that fails is the error, not its status number (1001)
    /// taken as the radius, which set a reach of `(2 / tan(45 deg) + 1) * 1001` across the
    /// edge. The solver's construction refuses a law with no radius at its middle, so the
    /// law is swapped after it; no public path reaches the failing branch, so this calls
    /// the crate function on a live session's solver.
    #[test]
    fn a_failed_law_evaluation_is_its_error_not_a_reach() {
        let mut k = Kernel::default();
        k.fixes.fillet_trace_reaches_rails = true;
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
        // the top edge at x = 0, 20 long
        let e = b
            .edges()
            .ids()
            .find(|&e| {
                let ivl = edge::interval(&b, e).unwrap();
                let c = edge::curve(&b, e).unwrap().unwrap();
                [ivl.min(), ivl.max()].into_iter().all(|t| {
                    let p = c.evaluate_point(&k, t).unwrap();
                    p.z == 30.0 && p.x == 0.0
                })
            })
            .unwrap();
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_variable_radius_edges(
                &mut k,
                &[e],
                2.0,
                FilletLaw::linear(2.0, 10.0, None),
                1,
                1,
            )
            .unwrap();
        let Some(&FilletObject::Solver(id)) = session.executive().solvers.first() else {
            panic!("no solver");
        };
        let objects = &mut session.objects;
        let solver = objects.solvers[id.0 as usize].as_mut().unwrap();
        let SolverObject::VariableRadius(vr) = &mut *solver else {
            panic!("not a variable-radius solver");
        };
        // a base law has no value anywhere (LawHasNoValue, SM_ERR = 1001)
        vr.law = FilletLaw::new(FilletLawKind::Base);
        let result = extend_trace_to_its_rails(&k, objects.target, solver);
        assert!(
            matches!(
                result.as_ref().map_err(SolidError::blend_failure),
                Err(Some(BlendFailure::LawHasNoValue))
            ),
            "{result:?}"
        );
        assert_eq!(solver.base().trace_reach, 0.0);
    }

    #[test]
    fn oblique_end_corner_sets_along_edge_trace_reach() {
        use crate::curve::curve::Curve;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::sweep::api_brep::create_linear_sweep;

        let mut k = Kernel::default();
        k.fixes.fillet_trace_reaches_rails = true;

        // Orthogonal box corner: no side edge runs backward past either end vertex.
        let mut bx = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
        let box_edge = bx
            .edges()
            .ids()
            .find(|&e| {
                let ivl = edge::interval(&bx, e).unwrap();
                let c = edge::curve(&bx, e).unwrap().unwrap();
                [ivl.min(), ivl.max()].into_iter().all(|t| {
                    let p = c.evaluate_point(&k, t).unwrap();
                    p.z == 30.0 && p.x == 0.0
                })
            })
            .unwrap();
        let mut box_session = FilletSession::new(&mut k, &mut bx).unwrap();
        box_session
            .prepare_constant_radius_edges(&mut k, &[box_edge], 1.0, 1, 1, 1.0)
            .unwrap();
        let Some(&FilletObject::Solver(box_sid)) = box_session.executive().solvers.first() else {
            panic!("no solver");
        };
        let box_objects = &mut box_session.objects;
        let box_solver = box_objects.solvers[box_sid.0 as usize].as_mut().unwrap();
        extend_trace_to_its_rails(&k, box_objects.target, box_solver).unwrap();
        assert_eq!(box_solver.base().trace_reach_along, 0.0);

        // Extruded polygon with a 3x45° chamfer meeting a sloped outer wall (-24,40)-(0,3):
        // cot(theta) = 61/13 ≈ 4.6923, exceeding half the chamfer's 4.2426 length.
        let pts = [
            Point3::new(-24.0, 40.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(45.0, 0.0, 0.0),
            Point3::new(45.0, 40.0, 0.0),
        ];
        let segs: Vec<Curve> = (0..pts.len())
            .map(|i| {
                Curve::from(
                    BSplineCurve::create_line_segment(3, pts[i], pts[(i + 1) % pts.len()]).unwrap(),
                )
            })
            .collect();
        let refs: Vec<Option<&Curve>> = segs.iter().map(Some).collect();
        let mut bar =
            create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), 50.0, true).unwrap();
        let chamfer_top = bar
            .edges()
            .ids()
            .find(|&e| {
                let ivl = edge::interval(&bar, e).unwrap();
                let c = edge::curve(&bar, e).unwrap().unwrap();
                let mid = c.evaluate_point(&k, ivl.evaluate(0.5)).unwrap();
                (mid.x - 1.5).abs() < 1e-4
                    && (mid.y - 1.5).abs() < 1e-4
                    && (mid.z - 50.0).abs() < 1e-4
            })
            .unwrap();
        let mut session = FilletSession::new(&mut k, &mut bar).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &[chamfer_top], 1.0, 1, 1, 1.0)
            .unwrap();
        let Some(&FilletObject::Solver(sid)) = session.executive().solvers.first() else {
            panic!("no solver");
        };
        let objects = &mut session.objects;
        let solver = objects.solvers[sid.0 as usize].as_mut().unwrap();
        extend_trace_to_its_rails(&k, objects.target, solver).unwrap();
        let expected_min = 2.0 * (61.0 / 13.0) + 1.0 - 1e-6;
        assert!(
            solver.base().trace_reach_along >= expected_min,
            "trace_reach_along = {}, expected >= {expected_min}",
            solver.base().trace_reach_along
        );
        assert!(
            solver.base().trace_along.iter().flatten().any(|&flag| flag),
            "trace_along = {:?}",
            solver.base().trace_along
        );
    }
}
