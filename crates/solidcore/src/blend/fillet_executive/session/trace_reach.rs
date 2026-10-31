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
//! `SmVariableRadiusFS::GetFilletRadius` yields its status code as the radius, as the
//! fork's `SER` inside that `double` function does (an `Unported` callee propagates).
//! A failed `GetOrCreateUVTrimCurve` skips that surface's across test, as the fork
//! does, but an `Unported` one propagates. There is no recursion and no unbounded
//! loop (7 and 65 fixed samples).

use crate::blend::fillet_executive::session::SolverObject;
use crate::error::{Result, SolidError, SM_ERR, SM_ERR_INVALID_INPUT};
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
use crate::topology::{edge, Brep};

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

/// The `SmStatus` the fork's `SER` returns for a failed callee.
fn status_of(e: &SolidError) -> i32 {
    match e {
        SolidError::Upstream { code, .. } => *code,
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => SM_ERR_INVALID_INPUT,
        _ => SM_ERR,
    }
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
/// `SM_ERR_NULL_POINTER` for a solver without its first edge use's edge or curve; those
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
    let edgeuse = base
        .edgeuse(0)?
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    let edge_id = eu::edge(target, edgeuse)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let curve = edge::curve(target, edge_id)?
        .ok_or_else(|| SolidError::null_pointer(OP))?
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
            // SmVariableRadiusFS::GetFilletRadius: SER inside a `double` function
            let r = match vr.law.evaluate(t, &law_ivl, vr.law_orientation) {
                Ok(values) => values.first().copied().unwrap_or(0.0),
                Err(e) if e.is_unported() => return Err(e),
                Err(e) => f64::from(status_of(&e)),
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

    // Each base surface's parameter directions that run across the edge (not mostly along
    // it, by the edge's direction on it in 3D proportions), and whether the default reaches.
    let mut across = [[true, true], [true, true]];
    let mut short = false;
    for ((surface, row), edgeuse) in bases.into_iter().zip(across.iter_mut()).zip(edgeuses) {
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
                Err(e) if e.is_unported() => return Err(e),
                Err(_) => continue,
            },
            Err(e) if e.is_unported() => return Err(e),
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
            for (flag, a) in row.iter_mut().zip(along) {
                *flag = a.abs() / len < 0.9;
            }
        }
    }
    if short {
        solver.base_mut().set_trace_reach(reach, across);
    }
    Ok(())
}
